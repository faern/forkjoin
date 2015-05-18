extern crate forkjoin;

use forkjoin::{FJData,TaskResult,ForkPool,AlgoStyle,ReduceStyle,Algorithm};

#[cfg(test)]
const FIB: Algorithm<usize, usize> = Algorithm {
    fun: fib_task,
    style: AlgoStyle::Reduce(ReduceStyle::NoArg(fib_join)),
};

#[test]
fn fib_40() {
    let forkpool = ForkPool::with_threads(4);
    let fibpool = forkpool.init_algorithm(FIB);

    let job = fibpool.schedule(40);
    let result: usize = job.recv().unwrap();
    assert_eq!(165580141, result);
}

#[test]
fn many_fib_15() {
    let n = 15;

    let forkpool = ForkPool::with_threads(4);
    let fibpool = forkpool.init_algorithm(FIB);

    let mut jobs = vec![];
    for _ in 0..100 {
        jobs.push(fibpool.schedule(n));
    }
    for job in jobs {
        let result: usize = job.recv().unwrap();
        //println!("fib({})={}", n, result);
        assert_eq!(987, result);
    }
}

#[test]
fn fast_after_slow() {
    let forkpool = ForkPool::with_threads(4);
    let fibpool = forkpool.init_algorithm(FIB);

    let long = fibpool.schedule(35);
    let short = fibpool.schedule(1);
    short.recv().unwrap();
    assert!(long.try_recv().is_err());
}

/// On my laptop fib(20) takes approx 32 us. parallell fib(0) takes approx 29 us.
/// Because of this I set serial threshold to 20 since that is where the serial execution
/// becomes the same speed as spawning one task.
#[cfg(test)]
fn fib_task(n: usize, fj: FJData) -> TaskResult<usize, usize> {
    if n <= 20 || fj.depth > fj.workers { // Example of cutoff to serial calculation
        TaskResult::Done(fib(n))
    } else {
        TaskResult::Fork(vec![n-1,n-2], None)
    }
}

#[cfg(test)]
fn fib_join(values: &[usize]) -> usize {
    values.iter().fold(0, |acc, &v| acc + v)
}

#[cfg(test)]
fn fib(n: usize) -> usize {
    if n < 2 {
        1
    } else {
        fib(n-1) + fib(n-2)
    }
}
