extern crate forkjoin;

use forkjoin::{TaskResult,ForkPool,AlgoStyle,SummaStyle,Algorithm};

#[cfg(test)]
const FIB: Algorithm<usize, usize> = Algorithm {
    fun: fib_task,
    style: AlgoStyle::Summa(SummaStyle::NoArg(fib_join)),
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
        assert_eq!(987, result);
    }
}

#[cfg(test)]
fn fib_task(n: usize) -> TaskResult<usize, usize> {
    if n < 10 {
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
