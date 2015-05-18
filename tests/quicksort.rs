extern crate forkjoin;
extern crate rand;

use forkjoin::{FJData,TaskResult,ForkPool,AlgoStyle,ReduceStyle,Algorithm};
use rand::Rng;

#[test]
fn quicksort_10() {
    let mut data: Vec<usize> = vec![10,1,8,3,9,2,7,4,6,5];

    quicksort_par(&mut data[..], 4);
    //quicksort_seq(&mut data[..]);

    assert_eq!(vec![1,2,3,4,5,6,7,8,9,10], data);
}

#[test]
fn quicksort_random() {
    let mut rng = rand::thread_rng();
    for _ in 0u32 .. 10000u32 {
            let len: usize = rng.gen();
            let mut v: Vec<usize> = rng.gen_iter::<usize>().take((len % 32) + 1).collect();
            quicksort_par(&mut v[..], 4);
            for i in 0 .. v.len() - 1 {
                assert!(v[i] <= v[i + 1])
            }
        }
}

#[cfg(test)]
fn quicksort_par(d: &mut[usize], threads: usize) {
    let forkpool = ForkPool::with_threads(threads);
    let sortpool = forkpool.init_algorithm(Algorithm {
        fun: quicksort_task,
        style: AlgoStyle::Reduce(ReduceStyle::NoArg(quicksort_join)),
    });
    let job = sortpool.schedule(&mut d[..]);
    job.recv().unwrap();
}

#[cfg(test)]
fn quicksort_task(d: &mut [usize], _: FJData) -> TaskResult<&mut [usize], ()> {
    let len = d.len();
    if len <= 1000 {
        quicksort_seq(d);
        TaskResult::Done(())
    } else {
        let pivot = partition(d);
        let (low, tmp) = d.split_at_mut(pivot);
        let (_, high) = tmp.split_at_mut(1);

        TaskResult::Fork(vec![low, high], None)
    }
}

#[cfg(test)]
fn quicksort_join(_: &[()]) -> () {}


#[cfg(test)]
fn quicksort_seq(d: &mut [usize]) {
    if d.len() > 1 {
        let pivot = partition(d);

        let (low, tmp) = d.split_at_mut(pivot);
        let (_, high) = tmp.split_at_mut(1);

        quicksort_seq(low);
        quicksort_seq(high);
    }
}

#[cfg(test)]
fn partition(d: &mut[usize]) -> usize {
    let last = d.len()-1;
    let pi = pick_pivot(d);
    let pv = d[pi];
    d.swap(pi, last); // Put pivot last
    let mut store = 0;
    for i in 0..last {
        if d[i] <= pv {
            d.swap(i, store);
            store += 1;
        }
    }
    if d[store] > pv {
        d.swap(store, last);
        store
    } else {
        last
    }
}

#[cfg(test)]
fn pick_pivot(d: &[usize]) -> usize {
    let len = d.len();
    if len < 3 {
        0
    } else {
        let is = [0, len/2, len-1];
        let mut vs = [d[0], d[len/2], d[len-1]];
        vs.sort();
        for i in is.iter() {
            if d[*i] == vs[1] {
                return *i;
            }
        }
        unreachable!();
    }
}
