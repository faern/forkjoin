// Copyright (c) 2015-2016 Linus Färnstrand.
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.


use std::sync::mpsc::{channel,Sender,Receiver};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize,Ordering};
use thread_scoped;
use deque::{self,Worker,Stealer};

use ::Task;
use ::workerthread::{WorkerThread};

/// Messages from `ForkPool` and `WorkerThread` to the `PoolSupervisor`.
pub enum SupervisorMsg<Arg: Send, Ret: Send + Sync> {
    /// The WorkerThreads use this to tell the `PoolSupervisor` they don't have anything
    /// to do and that stealing did not give any new `Task`s.
    /// The argument `usize` is the id of the `WorkerThread`.
    OutOfWork(usize),
    /// The `ForkPool` uses this to schedule new tasks on the `PoolSupervisor`.
    /// The `PoolSupervisor` will later schedule these to a `WorkerThread` when it see fit.
    Schedule(Task<Arg,Ret>),
    /// Message from the `ForkPool` to the `PoolSupervisor` to tell it to shutdown.
    Shutdown,
}

/// Internal handle to a WorkerThread
struct ThreadInfo<'thread> {
    channel: Sender<()>,
    #[allow(dead_code)] // Not used, only held for the join on drop effect.
    joinguard: thread_scoped::JoinGuard<'thread, ()>,
}

pub struct PoolSupervisorThread<'thread, Arg: Send, Ret: Send + Sync> {
    port: Receiver<SupervisorMsg<Arg, Ret>>,
    thread_infos: Vec<ThreadInfo<'thread>>,
    idle: usize,
    queue: Worker<Task<Arg, Ret>>,
    sleepers: Arc<AtomicUsize>,
}

impl<'t, Arg: Send + 't, Ret: Send + Sync + 't> PoolSupervisorThread<'t, Arg, Ret> {
    pub fn spawn(nthreads: usize) -> (Sender<SupervisorMsg<Arg,Ret>>, thread_scoped::JoinGuard<'t, ()>) {
        assert!(nthreads > 0);

        let (worker, stealer) = deque::new();

        let sleepers = Arc::new(AtomicUsize::new(0));
        let (worker_channel, supervisor_port) = channel();
        let thread_infos = PoolSupervisorThread::spawn_workers(
            nthreads,
            worker_channel.clone(),
            stealer,
            sleepers.clone(),);

        let joinguard = PoolSupervisorThread {
            port: supervisor_port,
            thread_infos: thread_infos,
            idle: nthreads,
            queue: worker,
            sleepers: sleepers,
        }.start_thread();

        (worker_channel, joinguard)
    }

    fn spawn_workers(nthreads: usize,
            worker_channel: Sender<SupervisorMsg<Arg,Ret>>,
            supervisor_stealer: Stealer<Task<Arg, Ret>>,
            sleepers: Arc<AtomicUsize>) -> Vec<ThreadInfo<'t>> {
        let mut threads = Vec::with_capacity(nthreads);
        let mut thread_channels = Vec::with_capacity(nthreads);
        let mut thread_stealers = Vec::with_capacity(nthreads);

        for id in 0..nthreads {
            let (supervisor_channel, worker_port) = channel();
            let thread: WorkerThread<Arg,Ret> = WorkerThread::new(
                id,
                worker_port,
                worker_channel.clone(),
                supervisor_stealer.clone(),
                sleepers.clone());
            let stealer = thread.get_stealer();

            threads.push(thread);
            thread_channels.push(supervisor_channel);
            thread_stealers.push(stealer);
        }

        for (i, thread) in threads.iter_mut().enumerate() {
            for (j, stealer) in thread_stealers.iter().enumerate() {
                if i != j {
                    thread.add_other_stealer(stealer.clone());
                }
            }
        }

        let mut thread_infos = Vec::with_capacity(nthreads);
        for (thread, supervisor_channel) in threads.into_iter().zip(thread_channels) {
            let joinguard = thread.spawn();
            thread_infos.push(ThreadInfo {
                channel: supervisor_channel,
                joinguard: joinguard,
            });
        }

        thread_infos
    }

    fn start_thread(self) -> thread_scoped::JoinGuard<'t, ()> {
        unsafe {
            thread_scoped::scoped(move|| {
                self.main_loop();
            })
        }
    }

    fn main_loop(mut self) {
        loop {
            match self.port.recv() {
                Err(_) => panic!("All WorkerThreads and the ForkPool closed their channels"),
                Ok(msg) => match msg {
                    SupervisorMsg::OutOfWork(_) => {
                        self.idle += 1;
                        if self.idle == self.thread_infos.len() {
                            if let Some(task) = self.queue.pop() {
                                self.queue.push(task);
                                self.msg_workers();
                            }
                        }
                    },
                    SupervisorMsg::Schedule(task) => {
                        self.queue.push(task);
                        if self.idle == self.thread_infos.len() {
                            self.msg_workers();
                        }
                    },
                    SupervisorMsg::Shutdown => break,
                },
            }
        }
    }

    fn msg_workers(&mut self) {
        self.idle = 0;
        self.sleepers.store(0, Ordering::SeqCst);
        for id in 0..self.thread_infos.len() {
            self.thread_infos[id].channel.send(()).unwrap();
        }
    }
}

// impl<'a, Arg: Send, Ret: Send + Sync> Drop for PoolSupervisorThread<'a, Arg, Ret> {
//     fn drop(&mut self) {
//         println!("Dropping PoolSupervisorThread");
//     }
// }
