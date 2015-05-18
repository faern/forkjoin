extern crate forkjoin;

use std::mem;
use forkjoin::{FJData,TaskResult,ForkPool,AlgoStyle,SummaStyle,Algorithm};

#[test]
fn mut_inc_one_to_five() {
    let mut data: Vec<usize> = vec![1,2,3,4,5];
    let unsafe_data: Vec<usize> = unsafe {
        let p = data.as_mut_ptr();
        Vec::from_raw_parts(p, data.len(), data.capacity())
    };

    let forkpool = ForkPool::with_threads(4);
    let mutpool = forkpool.init_algorithm(Algorithm {
        fun: mut_inc_task,
        style: AlgoStyle::Summa(SummaStyle::NoArg(mut_inc_join)),
    });

    let job = mutpool.schedule(&mut data[..]);
    job.recv().unwrap();

    assert_eq!(vec![2,3,4,5,6], unsafe_data);
    mem::forget(unsafe_data);
}

#[cfg(test)]
fn mut_inc_task(d: &mut [usize], _: FJData) -> TaskResult<&mut [usize], ()> {
    let len = d.len();
    if len == 1 {
        d[0] += 1;
        TaskResult::Done(())
    } else {
        let (d1,d2) = d.split_at_mut(len/2);
        TaskResult::Fork(vec![d1, d2], None)
    }
}

#[cfg(test)]
fn mut_inc_join(_: &[()]) -> () {}
