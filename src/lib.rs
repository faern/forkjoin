// Copyright 2015 Linus Färnstrand
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//!
//! A work stealing fork-join parallelism library.
//!
//! Inspired by the blog post [Data Parallelism in Rust](http://smallcultfollowing.com/babysteps/blog/2013/06/11/data-parallelism-in-rust/)
//! and implemented as part of a master's thesis.
//!
//! This library has been developed to accommodate the needs of three types of
//! algorithms that all fit very well for fork-join parallelism.
//!
//! # Summa style
//!
//! Summa style is where the algorithm receive an argument, recursively compute a value
//! from this argument and return one answer. Examples of this style include recursively
//! finding the n:th Fibonacci number and summing of tree structures.
//! Characteristics of this style is that the algorithm does not need to mutate its
//! argument and the resulting value is only available after every subtask has been
//! fully computed.
//!
//! ## Example of summa style
//!
//!     use forkjoin::{TaskResult,Fork,ForkPool,AlgoStyle};
//!
//!     fn fib_30_with_4_threads() {
//!         let forkpool = ForkPool::with_threads(4);
//!         let result_port = forkpool.schedule(fib_task, 30);
//!         let result: usize = result_port.recv().unwrap();
//!         assert_eq!(1346269, result);
//!     }
//!
//!     fn fib_task(n: usize) -> TaskResult<usize, usize> {
//!         if n < 2 {
//!             TaskResult::Done(1)
//!         } else {
//!             TaskResult::Fork(Fork{
//!                 fun: fib_task,
//!                 args: vec![n-1,n-2],
//!                 join: AlgoStyle::Summa(fib_join)})
//!         }
//!     }
//!
//!     fn fib_join(values: &[usize]) -> usize {
//!         values.iter().fold(0, |acc, &v| acc + v)
//!     }
//!
//! # Search style
//!
//! Search style return results continuously and can sometimes start without any
//! argument, or start with some initial state. The algorithm produce one or multiple
//! output values during the execution, possibly aborting anywhere in the middle.
//! Algorithms where leafs in the problem tree represent a complete solution to the
//! problem (Unless the leave represent a dead end that is not a solution and does
//! not spawn any subtasks), for example nqueens and sudoku solvers, have this style.
//! Characteristics of the search style is that they can produce multiple results
//! and can abort before all tasks in the tree have been computed.
//!
//! ## Example of search style
//!
//!     use forkjoin::{ForkPool,TaskResult,Fork,AlgoStyle};
//!
//!     type Queen = usize;
//!     type Board = Vec<Queen>;
//!     type Solutions = Vec<Board>;
//!
//!     fn search_nqueens() {
//!         let n: usize = 8;
//!         let empty = vec![];
//!
//!         let forkpool = ForkPool::with_threads(4);
//!         let par_solutions_port = forkpool.schedule(nqueens_task, (empty, n));
//!
//!         let mut solutions: Vec<Board> = vec![];
//!         loop {
//!             match par_solutions_port.recv() {
//!                 Err(..) => break, // Channel is closed to indicate termination
//!                 Ok(board) => solutions.push(board),
//!             };
//!         }
//!         let num_solutions = solutions.len();
//!         println!("Found {} solutions to nqueens({}x{})", num_solutions, n, n);
//!     }
//!
//!     fn nqueens_task((q, n): (Board, usize)) -> TaskResult<(Board,usize), Board> {
//!         if q.len() == n {
//!             TaskResult::Done(q)
//!         } else {
//!             let mut fork_args: Vec<(Board, usize)> = vec![];
//!             for i in 0..n {
//!                 let mut q2 = q.clone();
//!                 q2.push(i);
//!
//!                 if ok(&q2[..]) {
//!                     fork_args.push((q2, n));
//!                 }
//!             }
//!             TaskResult::Fork(Fork{
//!                 fun: nqueens_task,
//!                 args: fork_args,
//!                 join: AlgoStyle::Search
//!             })
//!         }
//!     }
//!
//!     fn ok(q: &[usize]) -> bool {
//!         for (x1, &y1) in q.iter().enumerate() {
//!             for (x2, &y2) in q.iter().enumerate() {
//!                 if x2 > x1 {
//!                     let xd = x2-x1;
//!                     if y1 == y2 || y1 == y2 + xd || (y2 >= xd && y1 == y2 - xd) {
//!                         return false;
//!                     }
//!                 }
//!             }
//!         }
//!         true
//!     }
//!
//! # In-place mutation style
//!
//! NOTE: This style works in the current lib version, but it requires very ugly
//! unsafe code!
//!
//! In-place mutation style receive a mutable argument, recursively modifies this value
//! and the result is the argument itself. Sorting algorithms that sort their input
//! arrays are cases of this style. Characteristics of this style is that they mutate
//! their input argument instead of producing any output.
//!
//! Examples of this will come when they can be nicely implemented.
//!
//! # Tasks
//!
//! The small jobs that are executed and can choose to fork or to return a value is the
//! TaskFun. A TaskFun can NEVER block, because that would block the kernel thread
//! it's being executed on. Instead it should decide if it's done calculating or need
//! to fork. This decision is taken in the return value to indicate to the user
//! that a TaskFun need to return before anything can happen.
//!
//! A TaskFun return a `TaskResult`. It can be `TaskResult::Done(value)` if it's done
//! calculating. It can be `TaskResult::Fork(fork)` if it needs to fork.


#![feature(unique)]
#![feature(scoped)]


extern crate deque;
extern crate rand;
extern crate num_cpus;

use std::ptr::Unique;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc,Mutex};
use std::sync::mpsc::{channel,Sender,Receiver};

mod workerthread;
mod poolsupervisor;

use ::poolsupervisor::PoolSupervisor;

/// Type definition of the main function in a task.
/// Your task functions must have this signature
pub type TaskFun<Arg, Ret> = extern "Rust" fn(Arg) -> TaskResult<Arg, Ret>;

/// Type definition of functions joining together forked results.
/// Only used in `AlgoStyle::Summa` algorithms.
pub type TaskJoin<Ret> = extern "Rust" fn(&[Ret]) -> Ret;

pub struct Task<Arg: Send, Ret: Send + Sync> {
    pub fun: TaskFun<Arg, Ret>,
    pub arg: Arg,
    pub join: ResultReceiver<Ret>,
}

/// Return values from tasks. Represent a computed value or a fork of the algorithm.
pub enum TaskResult<Arg, Ret> {
    /// Return this from `TaskFun` to indicate a computed value. Represents a leaf in the
    /// problem tree of the computation.
    ///
    /// If the algorithm style is `AlgoStyle::Search` the value in `Done` will be sent
    /// directly to the `Receiver<Ret>` held by the submitter of the computation.
    /// If the algorithm style is `AlgoStyle::Summa` the value in `Done` will be inserted
    /// into the `JoinBarrier` allocated by `ForkJoin`. If it's the last task to complete
    /// the join function will be executed.
    Done(Ret),

    /// Return this from `TaskFun` to indicate that the algorithm wants to fork.
    /// Takes a `Fork` instance describing how to fork.
    Fork(Fork<Arg, Ret>),
}

/// Struct describing how a `Task` want to fork into multiple subtasks.
pub struct Fork<Arg, Ret> {
    /// A function pointer. The function that will be executed by all the subtasks
    pub fun: TaskFun<Arg, Ret>,
    /// A list of the arguments to the subtasks. One subtask will be created for each argument.
    pub args: Vec<Arg>,
    /// Enum showing the type of algorithm, indicate what should be done with results from
    /// subtasks created by this fork.
    pub join: AlgoStyle<Ret>,
}

/// Enum representing the style of the executed algorithm.
pub enum AlgoStyle<Ret> {
    /// A `Summa` style algorithm join together the results of the individual nodes
    /// in the problem tree to finally form one result for the entire computation.
    ///
    /// Examples of this style include recursively computing fibbonacci numbers
    /// and summing binary trees.
    ///
    /// Takes a function pointer that joins together results as argument.
    Summa(TaskJoin<Ret>),

    /// A `Search` style algoritm return their results to the listener directly upon a
    /// `TaskResult::Done`.
    ///
    /// Examples of this style include sudoku solvers and nqueens where a node represent
    /// a complete solution.
    Search,
}

/// Internal struct for receiving results from multiple subtasks in parallel
pub struct JoinBarrier<Ret: Send + Sync> {
    /// Atomic counter counting missing arguments before this join can be executed.
    pub ret_counter: AtomicUsize,
    /// Function pointer to execute when all arguments have arrived.
    pub joinfun: TaskJoin<Ret>,
    /// Vector holding the results of all subtasks. Initialized unsafely so can't be used
    /// for anything until all the values have been put in place.
    pub joinfunarg: Vec<Ret>,
    /// Where to send the result of the execution of `joinfun`
    pub parent: ResultReceiver<Ret>,
}

/// Enum describing what to do with results of `Task`s and `JoinBarrier`s.
pub enum ResultReceiver<Ret: Send + Sync> {
    /// Algorithm has Summa style and the value should be inserted into a `JoinBarrier`
    Join(Unique<Ret>, Arc<JoinBarrier<Ret>>),
    /// Algorithm has Search style and results should be sent directly to the owner.
    Channel(Arc<Mutex<Sender<Ret>>>),
}

impl<Ret: Send + Sync> Clone for ResultReceiver<Ret> {
    fn clone(&self) -> Self {
        match *self {
            ResultReceiver::Join(..) => panic!("Unable to clone ResultReceiver::Join"),
            ResultReceiver::Channel(ref c) => ResultReceiver::Channel(c.clone()),
        }
    }
}

/// Messages from the `PoolSupervisor` to `WorkerThread`s
pub enum WorkerMsg<Arg: Send, Ret: Send + Sync> {
    /// A new `Task` to be scheduled for execution by the `WorkerThread`
    Schedule(Task<Arg,Ret>),
    /// Tell the `WorkerThread` to simply try to steal from the other `WorkerThread`s
    Steal,
}

/// Main struct of the ForkJoin library.
/// Represents a pool of threads implementing a work stealing algorithm.
pub struct ForkPool<'a, Arg: Send, Ret: Send + Sync> {
    supervisor: PoolSupervisor<'a, Arg, Ret>,
}

impl<'a, Arg: Send + 'a, Ret: Send + Sync + 'a> ForkPool<'a, Arg, Ret> {
    /// Create a new `ForkPool` using num_cpus to determine pool size
    ///
    /// On a X-core cpu with hyper-threading it creates 2X threads
    /// (4 core intel with HT results in 8 threads).
    /// This is not optimal. It makes the computer very slow and don't yield
    /// very much speedup compared to X threads. Not sure how to best fix this.
    /// Not very high priority.
    pub fn new() -> ForkPool<'a, Arg, Ret> {
        let nthreads = num_cpus::get();
        ForkPool::with_threads(nthreads)
    }

    /// Create a new `ForkPool` with `nthreads` `WorkerThread`s at its disposal.
    pub fn with_threads(nthreads: usize) -> ForkPool<'a, Arg, Ret> {
        assert!(nthreads > 0);
        let supervisor_channel = PoolSupervisor::new(nthreads);

        ForkPool {
            supervisor: supervisor_channel,
        }
    }

    /// Schedule a new computation on this `ForkPool`. Returns instantly.
    ///
    /// Return value(s) can be read from the returned `Receiver<Ret>`.
    /// `AlgoStyle::Summa` will only return one message on this channel.
    ///
    /// `AlgoStyle::Search` algorithm might return arbitrary number of messages.
    /// Algorithm termination is detected by the `Receiver<Ret>` returning an `Err`
    pub fn schedule(&self, fun: TaskFun<Arg, Ret>, arg: Arg) -> Receiver<Ret> {
        let (result_channel, result_port) = channel();

        let task = Task {
            fun: fun,
            arg: arg,
            join: ResultReceiver::Channel(Arc::new(Mutex::new(result_channel))),
        };
        self.supervisor.schedule(task);

        result_port
    }
}

// impl<'a, Arg: Send, Ret: Send + Sync> Drop for ForkPool<'a, Arg, Ret> {
//     fn drop(&mut self) {
//         println!("Dropping ForkPool");
//     }
// }
