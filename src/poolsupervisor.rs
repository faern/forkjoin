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


use std::sync::mpsc::{channel,Sender,Receiver};
use std::thread;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize,Ordering};
// use deque::{BufferPool,Worker,Stealer,Stolen};

use ::Task;
use ::workerthread::{WorkerThread,WorkerMsg};

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

struct ThreadInfo<'thread, Arg: Send, Ret: Send + Sync> {
    channel: Sender<WorkerMsg<Arg,Ret>>,
    #[allow(dead_code)] // Not used, only held for the join on drop effect.
    joinguard: thread::JoinGuard<'thread, ()>,
}

pub struct PoolSupervisorThread<'thread, Arg: Send, Ret: Send + Sync> {
    port: Receiver<SupervisorMsg<Arg, Ret>>,
    thread_infos: Vec<ThreadInfo<'thread, Arg, Ret>>,
    idle: usize,
    queue: Vec<Task<Arg,Ret>>,
    sleepers: Arc<AtomicUsize>,
}

impl<'t, Arg: Send + 't, Ret: Send + Sync + 't> PoolSupervisorThread<'t, Arg, Ret> {
    pub fn spawn(nthreads: usize) -> (Sender<SupervisorMsg<Arg,Ret>>, thread::JoinGuard<'t, ()>) {
        assert!(nthreads > 0);

        let sleepers = Arc::new(AtomicUsize::new(0));
        let (worker_channel, supervisor_port) = channel();
        let thread_infos = PoolSupervisorThread::spawn_workers(nthreads, worker_channel.clone(), sleepers.clone());

        let joinguard = PoolSupervisorThread {
            port: supervisor_port,
            thread_infos: thread_infos,
            idle: nthreads,
            queue: vec![],
            sleepers: sleepers,
        }.start_thread();

        (worker_channel, joinguard)
    }

    fn spawn_workers(nthreads: usize,
            worker_channel: Sender<SupervisorMsg<Arg,Ret>>,
            sleepers: Arc<AtomicUsize>) -> Vec<ThreadInfo<'t, Arg, Ret>> {
        let mut threads = Vec::with_capacity(nthreads);
        let mut thread_channels = Vec::with_capacity(nthreads);
        let mut thread_stealers = Vec::with_capacity(nthreads);

        for id in 0..nthreads {
            let (supervisor_channel, worker_port) = channel();
            let thread: WorkerThread<Arg,Ret> = WorkerThread::new(id, worker_port, worker_channel.clone(), sleepers.clone());
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

    fn start_thread(self) -> thread::JoinGuard<'t, ()> {
        let builder = thread::Builder::new().name(format!("fork-join supervisor"));
        let joinguard = builder.scoped(move|| {
            self.main_loop();
        });
        joinguard.unwrap()
    }

    fn main_loop(mut self) {
        loop {
            match self.port.recv() {
                Err(_) => panic!("All WorkerThreads and the ForkPool closed their channels"),
                Ok(msg) => match msg {
                    SupervisorMsg::OutOfWork(_) => {
                        self.idle += 1;
                        if self.idle == self.thread_infos.len() {
                            if !self.queue.is_empty() {
                                self.idle = 0;
                                self.sleepers.store(0, Ordering::SeqCst);
                                let task = self.queue.remove(0);
                                self.schedule(task);
                            }
                        }
                    },
                    SupervisorMsg::Schedule(task) => {
                        if self.idle == self.thread_infos.len() {
                            self.idle = 0;
                            self.sleepers.store(0, Ordering::SeqCst);
                            self.schedule(task);
                        } else {
                            self.queue.push(task);
                        }
                    },
                    SupervisorMsg::Shutdown => break,
                },
            }
        }
    }

    fn schedule(&self, task: Task<Arg, Ret>) {
        self.thread_infos[0].channel.send(WorkerMsg::Schedule(task)).unwrap();
        for id in 1..self.thread_infos.len() {
            self.thread_infos[id].channel.send(WorkerMsg::Steal).unwrap();
        }
    }
}

// impl<'a, Arg: Send, Ret: Send + Sync> Drop for PoolSupervisorThread<'a, Arg, Ret> {
//     fn drop(&mut self) {
//         println!("Dropping PoolSupervisorThread");
//     }
// }
