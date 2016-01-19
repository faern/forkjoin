// Copyright (c) 2015-2016 Linus Färnstrand.
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

extern crate forkjoin;

use std::mem;
use forkjoin::{TaskResult,ForkPool,AlgoStyle,ReduceStyle,Algorithm};

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
        style: AlgoStyle::Reduce(ReduceStyle::NoArg(mut_inc_join)),
    });

    let job = mutpool.schedule(&mut data[..]);
    job.recv().unwrap();

    assert_eq!(vec![2,3,4,5,6], unsafe_data);
    mem::forget(unsafe_data);
}

#[cfg(test)]
fn mut_inc_task(d: &mut [usize]) -> TaskResult<&mut [usize], ()> {
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
