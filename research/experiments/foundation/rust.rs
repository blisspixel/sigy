use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Instant;

fn main() {
    let budget = Arc::new(Mutex::new(10_000_u64));
    let mut workers = Vec::new();
    for _ in 0..16 {
        let budget = Arc::clone(&budget);
        workers.push(thread::spawn(move || {
            let mut accepted = 0_u64;
            for _ in 0..1_000 {
                let mut available = budget.lock().expect("budget mutex");
                if *available >= 13 {
                    *available -= 13;
                    accepted += 1;
                }
            }
            accepted
        }));
    }
    let accepted: u64 = workers.into_iter().map(|w| w.join().expect("worker")).sum();
    assert_eq!(accepted, 769);
    assert_eq!(*budget.lock().expect("budget mutex"), 3);

    let started = Instant::now();
    let (send, receive) = mpsc::sync_channel::<Vec<u8>>(64);
    let consumer = thread::spawn(move || {
        let mut sum = 0_u64;
        let mut count = 0_u64;
        for block in receive {
            sum += block.iter().map(|v| u64::from(*v)).sum::<u64>();
            count += 1;
        }
        (sum, count)
    });
    let mut expected = 0_u64;
    for i in 0_u32..20_000 {
        let value = u8::try_from(i % 251).expect("bounded byte");
        expected += u64::from(value) * 4096;
        send.send(vec![value; 4096]).expect("consumer alive");
    }
    drop(send);
    assert_eq!(consumer.join().expect("consumer"), (expected, 20_000));
    let elapsed = started.elapsed().as_micros();

    let (send, receive) = mpsc::sync_channel::<u8>(1);
    send.send(1).expect("queue capacity");
    let cancelled = thread::spawn(move || send.send(2).is_err());
    drop(receive);
    assert!(cancelled.join().expect("cancelled sender"));
    println!("accepted={accepted} remaining=3 blocks=20000 bytes=81920000 queue_us={elapsed} cancellation=pass");
}
