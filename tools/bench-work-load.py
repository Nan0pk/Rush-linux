#!/usr/bin/env python3
import sys
import time
import os
import signal
import multiprocessing

def run_worker(worker_id):
    shm_path = f"/dev/shm/optid-bench-work-{worker_id}"
    c = 0
    
    def handler(signum, frame):
        try:
            with open(shm_path, "w") as f:
                f.write(str(c))
        except:
            pass
        sys.exit(0)
        
    signal.signal(signal.SIGTERM, handler)
    signal.signal(signal.SIGINT, handler)
    
    last_write = 0
    while True:
        c += 1
        if c - last_write >= 1000000:
            try:
                with open(shm_path, "w") as f:
                    f.write(str(c))
                last_write = c
            except:
                pass

def main():
    if len(sys.argv) < 2:
        print("Usage: bench-work-load.py <threads>")
        sys.exit(1)
        
    num_threads = int(sys.argv[1])
    
    # Clean up old files
    for i in range(128):
        try:
            os.remove(f"/dev/shm/optid-bench-work-{i}")
        except FileNotFoundError:
            pass
            
    processes = []
    for i in range(num_threads):
        p = multiprocessing.Process(target=run_worker, args=(i,))
        p.start()
        processes.append(p)
        
    def term_main(signum, frame):
        for p in processes:
            if p.is_alive():
                os.kill(p.pid, signal.SIGTERM)
        sys.exit(0)
        
    signal.signal(signal.SIGTERM, term_main)
    signal.signal(signal.SIGINT, term_main)
    
    for p in processes:
        p.join()

if __name__ == "__main__":
    main()
