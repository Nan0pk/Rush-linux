// S3D systemd notification and transaction-journal health gate.

use std::env;
use std::ffi::OsStr;
#[cfg(target_os = "linux")]
use std::os::linux::net::SocketAddrExt;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{SocketAddr, UnixDatagram};
use std::sync::atomic::{AtomicBool, Ordering};

static READY_SENT: AtomicBool = AtomicBool::new(false);

fn verify_journal_health(
    transactions: &TransactionEngine,
    io: &dyn KernelIo,
) -> Result<(), TransactionError> {
    if !io.exists(&transactions.root) {
        return Ok(());
    }
    for path in io
        .read_dir(&transactions.root)
        .map_err(|error| TransactionError::io("scan recovery directory", error))?
    {
        let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("");
        if name.starts_with('.') && name.ends_with(".tmp") {
            return Err(TransactionError::new(
                TransactionErrorKind::InvalidRecord,
                format!("unpublished transaction temp file remains: {}", path.display()),
            ));
        }
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let record = transactions.load_record(io, &path)?;
        transactions.validate_generation_and_identity(io, &record)?;
        if record.phase != TransactionPhase::Committed {
            return Err(TransactionError::new(
                TransactionErrorKind::PhaseConflict,
                format!(
                    "{} remains in {:?} after a completed cycle",
                    record.target_id, record.phase
                ),
            ));
        }
    }
    Ok(())
}

fn notify_socket(message: &str) -> io::Result<()> {
    let Some(raw) = env::var_os("NOTIFY_SOCKET") else {
        return Ok(());
    };
    let socket = UnixDatagram::unbound()?;
    let bytes = OsStr::new(&raw).as_bytes();
    if bytes.first() == Some(&b'@') {
        #[cfg(target_os = "linux")]
        {
            let address = SocketAddr::from_abstract_name(&bytes[1..])?;
            socket.send_to_addr(message.as_bytes(), &address)?;
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "abstract systemd notification sockets require Linux",
            ));
        }
    }
    socket.send_to(message.as_bytes(), Path::new(&raw))?;
    Ok(())
}

#[cfg(test)]
thread_local! {
    static TEST_NOTIFICATIONS: std::cell::RefCell<Option<std::sync::Arc<std::sync::Mutex<Vec<String>>>>> =
        const { std::cell::RefCell::new(None) };
}

fn emit(message: &str) -> io::Result<()> {
    #[cfg(test)]
    {
        let captured = TEST_NOTIFICATIONS.with(|slot| slot.borrow().clone());
        if let Some(captured) = captured {
            captured
                .lock()
                .expect("S3D notification capture mutex poisoned")
                .push(message.to_string());
            return Ok(());
        }
    }
    notify_socket(message)
}

fn notify_cycle_complete(
    transactions: &TransactionEngine,
    io: &dyn KernelIo,
) -> Result<(), TransactionError> {
    verify_journal_health(transactions, io)?;
    let first = READY_SENT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok();
    let message = if first {
        "READY=1\nSTATUS=optid control cycle complete\nWATCHDOG=1"
    } else {
        "WATCHDOG=1"
    };
    emit(message).map_err(|error| TransactionError::io("notify systemd watchdog", error))
}

#[cfg(test)]
fn capture_notifications<T>(run: impl FnOnce() -> T) -> (T, Vec<String>) {
    let capture = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    READY_SENT.store(false, Ordering::SeqCst);
    TEST_NOTIFICATIONS.with(|slot| {
        slot.replace(Some(capture.clone()));
    });
    let result = run();
    TEST_NOTIFICATIONS.with(|slot| {
        slot.replace(None);
    });
    let messages = capture
        .lock()
        .expect("S3D notification capture mutex poisoned")
        .clone();
    (result, messages)
}
