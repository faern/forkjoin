extern crate forkjoin;

use std::mem;
use forkjoin::{TaskResult,Fork,ForkPool,JoinStyle,SummaStyle};

#[test]
fn mut_inc_one_to_five() {
    let mut data: Vec<usize> = vec![1,2,3,4,5];
    let unsafe_data: Vec<usize> = unsafe {
        let p = data.as_mut_ptr();
        Vec::from_raw_parts(p, data.len(), data.capacity())
    };

    let forkpool = ForkPool::with_threads(4);

    let result_port = forkpool.schedule(mut_inc_task, &mut data[..]);
    result_port.recv().unwrap();

    assert_eq!(vec![2,3,4,5,6], unsafe_data);
    unsafe { mem::forget(unsafe_data); }
}

#[cfg(test)]
fn mut_inc_task(d: &mut [usize]) -> TaskResult<&mut [usize], ()> {
    let len = d.len();
    if len == 1 {
        d[0] += 1;
        TaskResult::Done(())
    } else {
        let (d1,d2) = d.split_at_mut(len/2);
        TaskResult::Fork(Fork {
            fun: mut_inc_task,
            args: vec![d1, d2],
            join: JoinStyle::Summa(SummaStyle::JustJoin(mut_inc_join))
        })
    }
}

#[cfg(test)]
fn mut_inc_join(_: &[()]) -> () {}
