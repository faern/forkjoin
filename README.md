# ForkJoin
A work stealing fork-join parallelism library.

[![Build Status](https://api.travis-ci.org/faern/forkjoin.svg?branch=master)](https://travis-ci.org/faern/forkjoin)

Inspired by the blog post [Data Parallelism in Rust](http://smallcultfollowing.com/babysteps/blog/2013/06/11/data-parallelism-in-rust/)
and implemented as part of a master's thesis.

This library has been developed to accommodate the needs of three types of
algorithms that all fit very well for fork-join parallelism.

# Summa style

Summa style is where the algorithm receive an argument, recursively compute a value
from this argument and return one answer. Examples of this style include recursively
finding the n:th Fibonacci number and summing of tree structures.
Characteristics of this style is that the algorithm does not need to mutate its
argument and the resulting value is only available after every subtask has been
fully computed.

## Example of summa style

    use forkjoin::{TaskResult,Fork,ForkPool,AlgoStyle};

    fn fib_30_with_4_threads() {
        let forkpool = ForkPool::with_threads(4);
        let result_port = forkpool.schedule(fib_task, 30);
        let result: usize = result_port.recv().unwrap();
        assert_eq!(1346269, result);
    }

    fn fib_task(n: usize) -> TaskResult<usize, usize> {
        if n < 2 {
            TaskResult::Done(1)
        } else {
            TaskResult::Fork(Fork{
                fun: fib_task,
                args: vec![n-1,n-2],
                join: AlgoStyle::Summa(fib_join)})
        }
    }

    fn fib_join(values: &[usize]) -> usize {
        values.iter().fold(0, |acc, &v| acc + v)
    }

# Search style

Search style return results continuously and can sometimes start without any
argument, or start with some initial state. The algorithm produce one or multiple
output values during the execution, possibly aborting anywhere in the middle.
Algorithms where leafs in the problem tree represent a complete solution to the
problem (Unless the leave represent a dead end that is not a solution and does
not spawn any subtasks), for example nqueens and sudoku solvers, have this style.
Characteristics of the search style is that they can produce multiple results
and can abort before all tasks in the tree have been computed.

## Example of search style

    use forkjoin::{ForkPool,TaskResult,Fork,AlgoStyle};

    type Queen = usize;
    type Board = Vec<Queen>;
    type Solutions = Vec<Board>;

    fn search_nqueens() {
        let n: usize = 8;
        let empty = vec![];

        let forkpool = ForkPool::with_threads(4);
        let par_solutions_port = forkpool.schedule(nqueens_task, (empty, n));

        let mut solutions: Vec<Board> = vec![];
        loop {
            match par_solutions_port.recv() {
                Err(..) => break, // Channel is closed to indicate termination
                Ok(board) => solutions.push(board),
            };
        }
        let num_solutions = solutions.len();
        println!("Found {} solutions to nqueens({}x{})", num_solutions, n, n);
    }

    fn nqueens_task((q, n): (Board, usize)) -> TaskResult<(Board,usize), Board> {
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
            TaskResult::Fork(Fork{
                fun: nqueens_task,
                args: fork_args,
                join: AlgoStyle::Search
            })
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

# In-place mutation style

NOTE: This style works in the current lib version, but it requires very ugly
unsafe code!

In-place mutation style receive a mutable argument, recursively modifies this value
and the result is the argument itself. Sorting algorithms that sort their input
arrays are cases of this style. Characteristics of this style is that they mutate
their input argument instead of producing any output.

Examples of this will come when they can be nicely implemented.

# Tasks

The small jobs that are executed and can choose to fork or to return a value is the TaskFun. A TaskFun can NEVER block, because that would block the kernel thread it's being executed on. Instead it should decide if it's done calculating or need to fork. This decision is taken in the return value to indicate to the user that a TaskFun need to return before anything can happen.

A TaskFun return a `TaskResult`. It can be `TaskResult::Done(value)` if it's done calculating. It can be `TaskResult::Fork(fork)` if it needs to fork.

# TODO

* Make the mutation style algorithms work cleaner (without unsafe).
* Make it work in beta.
