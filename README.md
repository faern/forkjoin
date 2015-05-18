# ForkJoin
A work stealing fork-join parallelism library.

[![Build Status](https://api.travis-ci.org/faern/forkjoin.svg?branch=master)](https://travis-ci.org/faern/forkjoin)

Inspired by the blog post [Data Parallelism in Rust](http://smallcultfollowing.com/babysteps/blog/2013/06/11/data-parallelism-in-rust/)
and implemented as part of a master's thesis. Repository hosted at [github.com/faern/forkjoin](https://github.com/faern/forkjoin)

Library documentation hosted [here](https://faern.github.io/rust-docs/forkjoin/forkjoin/)

This library has been developed to accommodate the needs of three types of
algorithms that all fit very well for fork-join parallelism.

# Summa style

Summa style is where the algorithm receive an argument, recursively compute a value
from this argument and return one answer. Examples of this style include recursively
finding the n:th Fibonacci number and summing of tree structures.
Characteristics of this style is that the algorithm does not need to mutate its
argument and the resulting value is only available after every subtask has been
fully computed.

In summa style algorithms the return values of each subtask is passed to a special
join function that is executed when all subtasks have completed.
To this join function an extra argument can be sent directly from the task if the algorithm
has has `SummaStyle::Arg`. This can be seen in the examples here.

## Example of summa style (`SummaStyle::NoArg`)

```rust
use forkjoin::{FJData,TaskResult,ForkPool,AlgoStyle,SummaStyle,Algorithm};

fn fib_30_with_4_threads() {
    let forkpool = ForkPool::with_threads(4);
    let fibpool = forkpool.init_algorithm(Algorithm {
        fun: fib_task,
        style: AlgoStyle::Summa(SummaStyle::NoArg(fib_join)),
    });

    let job = fibpool.schedule(30);
    let result: usize = job.recv().unwrap();
    assert_eq!(1346269, result);
}

fn fib_task(n: usize, _: FJData) -> TaskResult<usize, usize> {
    if n < 2 {
        TaskResult::Done(1)
    } else {
        TaskResult::Fork(vec![n-1,n-2], None)
    }
}

fn fib_join(values: &[usize]) -> usize {
    values.iter().fold(0, |acc, &v| acc + v)
}
```

## Example of summa style (`SummaStyle::Arg`)

```rust
use forkjoin::{FJData,TaskResult,ForkPool,AlgoStyle,SummaStyle,Algorithm};

struct Tree {
    value: usize,
    children: Vec<Tree>,
}

fn sum_tree(t: &Tree) -> usize {
    let forkpool = ForkPool::new();
    let sumpool = forkpool.init_algorithm(Algorithm {
        fun: sum_tree_task,
        style: AlgoStyle::Summa(SummaStyle::Arg(sum_tree_join)),
    });
    let job = sumpool.schedule(t);
    job.recv().unwrap()
}

fn sum_tree_task(t: &Tree, fj: FJData) -> TaskResult<&Tree, usize> {
    if t.children.is_empty() {
        TaskResult::Done(t.value)
    } else if fj.depth > fj.workers { // Bad example of serial threshold
        TaskResult::Done(sum_tree_seq(t))
    } else {
        let mut fork_args: Vec<&Tree> = vec![];
        for c in t.children.iter() {
            fork_args.push(c);
        }
        TaskResult::Fork(fork_args, Some(t.value)) // Pass current nodes value to join
    }
}

fn sum_tree_seq(t: &Tree) -> usize {
    t.value + t.children.iter().fold(0, |acc, t2| acc + sum_tree_seq(t2))
}

fn sum_tree_join(value: &usize, values: &[usize]) -> usize {
    *value + values.iter().fold(0, |acc, &v| acc + v)
}
```

# Search style

Search style return results continuously and can sometimes start without any
argument, or start with some initial state. The algorithm produce one or multiple
output values during the execution, possibly aborting anywhere in the middle.
Algorithms where leafs in the problem tree represent a complete solution to the
problem (unless the leaf represent a dead end that is not a solution and does
not spawn any subtasks), for example nqueens and sudoku solvers, have this style.
Characteristics of the search style is that they can produce multiple results
and can abort before all tasks in the tree have been computed.

## Example of search style

```rust
use forkjoin::{FJData,ForkPool,TaskResult,AlgoStyle,Algorithm};

type Queen = usize;
type Board = Vec<Queen>;
type Solutions = Vec<Board>;

fn search_nqueens() {
    let n: usize = 8;
    let empty = vec![];

    let forkpool = ForkPool::with_threads(4);
    let queenpool = forkpool.init_algorithm(Algorithm {
        fun: nqueens_task,
        style: AlgoStyle::Search,
    });

    let job = queenpool.schedule((empty, n));

    let mut solutions: Vec<Board> = vec![];
    loop {
        match job.recv() {
            Err(..) => break, // Job has completed
            Ok(board) => solutions.push(board),
        };
    }
    let num_solutions = solutions.len();
    println!("Found {} solutions to nqueens({}x{})", num_solutions, n, n);
}

fn nqueens_task((q, n): (Board, usize), fj: FJData) -> TaskResult<(Board,usize), Board> {
    if q.len() == n {
        TaskResult::Done(q)
    } else {
        let mut fork_args: Vec<(Board, usize)> = vec![];
        for i in 0..n {
            let mut q2 = q.clone();
            q2.push(i);

            if ok(&q2[..]) {
                fork_args.push((q2, n));
            }
        }
        TaskResult::Fork(fork_args, None)
    }
}

fn ok(q: &[usize]) -> bool {
    for (x1, &y1) in q.iter().enumerate() {
        for (x2, &y2) in q.iter().enumerate() {
            if x2 > x1 {
                let xd = x2-x1;
                if y1 == y2 || y1 == y2 + xd || (y2 >= xd && y1 == y2 - xd) {
                    return false;
                }
            }
        }
    }
    true
}
```

# In-place mutation style

NOTE: This style works in the current lib version, but it requires very ugly
unsafe code!

In-place mutation style receive a mutable argument, recursively modifies this value
and the result is the argument itself. Sorting algorithms that sort their input
arrays are cases of this style. Characteristics of this style is that they mutate
their input argument instead of producing any output.

Examples of this will come when they can be nicely implemented.

# Tasks

The small units that are executed and can choose to fork or to return a value is the
`TaskFun`. A TaskFun can NEVER block, because that would block the kernel thread
it's being executed on. Instead it should decide if it's done calculating or need
to fork. This decision is taken in the return value to indicate to the user
that a TaskFun need to return before anything can happen.

A TaskFun return a `TaskResult`. It can be `TaskResult::Done(value)` if it's done
calculating. It can be `TaskResult::Fork(args)` if it needs to fork.

# TODO

- [ ] Make mutation style algorithms work without giving join function
- [ ] Implement a sorting algorithm. Quicksort?
- [ ] Remove need to return None on fork with NoArg
- [ ] Make it possible to use algorithms with different Arg & Ret on same pool.
- [ ] Make ForkJoin work in stable Rust.
- [ ] Remove mutex around channel in search style.
