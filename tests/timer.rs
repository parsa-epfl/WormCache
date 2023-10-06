use spinlock::Spinlock;
use std::arch::asm;
use std::sync::Arc;
use std::time::Instant;
/// This test is critical to examine whether the timer is atomic and monotonic.
/// The high-level idea is very simple: There is a FIFO protected by a lock, and each thread acquires the lock, pushes the timestamp it reads, and releases the lock.
/// Finally the main threads pops the timestamps and checks whether they are monotonic.
use std::time::SystemTime;

fn normal_time_function() -> u64 {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    now.as_nanos() as u64
}

// This function is only available on x86 and x86_64 platform.
#[cfg(target_arch = "x86_64")]
fn retsc_time_function() -> u64 {
    let time: u64;
    unsafe {
        asm!("rdtsc", out("rax") time);
    }
    time
}

#[test]
#[cfg(target_arch = "x86_64")]
fn test_wiether_tsc_is_atomic() {
    let mut time_list: Vec<u64> = Vec::new();
    for _ in 0..1000 {
        time_list.push(retsc_time_function());
    }
    for i in 0..time_list.len() - 1 {
        assert!(time_list[i] < time_list[i + 1]);
        println!("{} {}", time_list[i], time_list[i + 1]);
    }
}

#[test]
fn test_whether_timer_atomic() {
    // let me try whether the rdstic timer on the single thread is monotonic.

    const TOTAL_TEST_COUNT: usize = 1000_000;
    const TOTAL_THREAD_COUNT: usize = 16;

    let time_list = Arc::new(Spinlock::new(Vec::<u64>::with_capacity(
        TOTAL_TEST_COUNT * TOTAL_THREAD_COUNT,
    )));

    // spawn 16 threads
    let mut thread_list = Vec::new();
    for _ in 0..TOTAL_THREAD_COUNT {
        let time_list = time_list.clone();
        thread_list.push(std::thread::spawn(move || {
            for _ in 0..TOTAL_TEST_COUNT {
                let mut time_list = time_list.lock();
                time_list.push(normal_time_function());
            }
        }));
    }

    // wait for all threads to finish
    for thread in thread_list {
        thread.join().unwrap();
    }

    // verify whether value is monotonic
    let time_list = time_list.lock();
    for i in 0..time_list.len() - 1 {
        assert!(time_list[i] <= time_list[i + 1]);
        // println!("{} {}", time_list[i], time_list[i + 1]);
    }
}

#[test]
fn the_cost_of_timer() {
    // Get the average latency of calling the timer function.
    const TOTAL_TEST_COUNT: usize = 100_000_000;
    const THREAD_COUNT: usize = 16;

    let handlers: Vec<_> = (0..THREAD_COUNT).map(|_| {
        let handler = std::thread::spawn(|| {
            let mut fake_number: u64 = 0;
            let start = Instant::now();
            for _ in 0..TOTAL_TEST_COUNT {
                fake_number |= normal_time_function();
            }
            let end = start.elapsed();
            if fake_number == 0 {
                println!("This is a fake number: {}", fake_number);
            }

            return end;
        });
        return handler;
    }).collect();

    // sum all durations from each thread
    let acc = handlers.into_iter().fold(0, |acc, handler| {
        let end = handler.join().unwrap();
        return acc + end.as_nanos();
    });


    println!(
        "Average time: {} ns",
        acc / (TOTAL_TEST_COUNT * THREAD_COUNT) as u128
    );
}
