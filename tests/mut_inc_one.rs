extern crate forkjoin;

use std::mem;
use forkjoin::{TaskResult,Fork,ForkPool,AlgoStyle};

#[test]
fn mut_inc_one_to_five() {
    let mut data: Vec<usize> = vec![1,2,3,4,5];
    let unsafe_data: Vec<usize> = unsafe {
        let p = data.as_mut_ptr();
        Vec::from_raw_parts(p, data.len(), data.capacity())
    };

    let forkpool = ForkPool::with_threads(4);

    let result_port = forkpool.schedule(mut_inc_task, data);
    result_port.recv().unwrap();

    assert_eq!(vec![2,3,4,5,6], unsafe_data);
}

#[cfg(test)]
fn mut_inc_task(d: Vec<usize>) -> TaskResult<Vec<usize>, ()> {
    let mut data = d;
    let len = data.len();
    if len == 1 {
        data[0] += 1;
        TaskResult::Done(())
    } else {
        let l1 = len/2;
        let l2 = len-l1;
        let p = data.as_mut_ptr();
        unsafe {
            mem::forget(data);
            let v1: Vec<usize> = Vec::from_raw_parts(p, l1, l1);
            let v2: Vec<usize> = Vec::from_raw_parts(p.offset(l1 as isize), l2, l2);
            TaskResult::Fork(Fork {
                fun: mut_inc_task,
                args: vec![v1,v2],
                join: AlgoStyle::Summa(mut_inc_join, ())
            })
        }
    }
}

#[cfg(test)]
fn mut_inc_join(_: &(), _: &[()]) -> () {}
