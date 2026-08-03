#[test]
fn s3d_complete_cycle_emits_ready_and_watchdog() {
    let kernel = MemoryKernel::new();
    let root = PathBuf::from("/state/s3d-recovery");
    kernel.add_dir(Path::new("/state"), &root);
    let transactions = TransactionEngine::new(root, "generation".to_string());

    let (result, messages) =
        capture_notifications(|| notify_cycle_complete(&transactions, &kernel));

    result.expect("healthy completed cycle must notify systemd");
    assert_eq!(
        messages,
        vec!["READY=1\nSTATUS=optid control cycle complete\nWATCHDOG=1"]
    );
}

#[test]
fn s3d_journal_failure_withholds_watchdog() {
    let kernel = MemoryKernel::new();
    let root = PathBuf::from("/state/s3d-recovery");
    kernel.add_dir(Path::new("/state"), &root);
    let temp = root.join(".unpublished.json.generation.tmp");
    kernel.write_raw(&temp, "partial");
    kernel.add_dir_entry(&root, &temp);
    let transactions = TransactionEngine::new(root, "generation".to_string());

    let (result, messages) =
        capture_notifications(|| notify_cycle_complete(&transactions, &kernel));

    assert!(result.is_err());
    assert!(messages.is_empty());
}
