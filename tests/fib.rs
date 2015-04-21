extern crate forkjoin;

use forkjoin::{TaskResult,Fork,ForkPool,AlgoStyle,SummaStyle, Algorithm};

#[test]
fn fib_40() {
    let forkpool = ForkPool::with_threads(4);
    let result_port = forkpool.schedule(fib_task, 40);
    let result: usize = result_port.recv().unwrap();
    assert_eq!(165580141, result);
}

#[test]
fn many_fib_15() {
    let n = 15;

    let forkpool = ForkPool::with_threads(4);

    let mut ports = vec![];
    for _ in 0..100 {
        ports.push(forkpool.schedule(fib_task, n));
    }
    for port in ports {
        let result: usize = port.recv().unwrap();
        assert_eq!(987, result);
    }
}

#[cfg(test)]
struct Fib;

impl Algorithm for Fib {
    fn get_fun() -> TaskFun<Arg, Ret> {
        fib_task
    }

    fn get_style() -> AlgoStyle<Ret> {
        AlgoStyle::Summa(SummaStyle::NoArg(fib_join))
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
