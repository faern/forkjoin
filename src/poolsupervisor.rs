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

use ::{Task,WorkerMsg};
use ::workerthread::WorkerThread;

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

pub struct PoolSupervisor<'a, Arg: Send, Ret: Send + Sync> {
    joinguard: thread::JoinGuard<'a, ()>,
    channel: Sender<SupervisorMsg<Arg, Ret>>,
}

impl<'a, Arg: Send + 'a, Ret: Send + Sync + 'a> PoolSupervisor<'a, Arg, Ret> {
    pub fn new(nthreads: usize) -> PoolSupervisor<'a, Arg, Ret> {
        let (channel, joinguard) = PoolSupervisorThread::new(nthreads);

        PoolSupervisor {
            joinguard: joinguard,
            channel: channel,
        }
    }

    pub fn schedule(&self, task: Task<Arg, Ret>) {
        self.channel.send(SupervisorMsg::Schedule(task)).unwrap();
    }
}

impl<'a, Arg: Send, Ret: Send + Sync> Drop for PoolSupervisor<'a, Arg, Ret> {
    fn drop(&mut self) {
        //println!("Dropping PoolSupervisor");
        self.channel.send(SupervisorMsg::Shutdown).unwrap();
    }
}

struct ThreadInfo<'a, Arg: Send, Ret: Send + Sync> {
    channel: Sender<WorkerMsg<Arg,Ret>>,
    joinguard: thread::JoinGuard<'a, ()>,
}

pub struct PoolSupervisorThread<'a, Arg: Send, Ret: Send + Sync> {
    port: Receiver<SupervisorMsg<Arg, Ret>>,
    thread_infos: Vec<ThreadInfo<'a, Arg, Ret>>,
}

impl<'a, Arg: Send + 'a, Ret: Send + Sync + 'a> PoolSupervisorThread<'a, Arg, Ret> {
    pub fn new(nthreads: usize) -> (Sender<SupervisorMsg<Arg,Ret>>, thread::JoinGuard<'a, ()>) {
        assert!(nthreads > 0);

        let (worker_channel, supervisor_port) = channel();
        let thread_infos = PoolSupervisorThread::spawn_workers(nthreads, worker_channel.clone());

        let joinguard = PoolSupervisorThread {
            port: supervisor_port,
            thread_infos: thread_infos,
        }.spawn();

        (worker_channel, joinguard)
    }

    fn spawn_workers(nthreads: usize, worker_channel: Sender<SupervisorMsg<Arg,Ret>>) -> Vec<ThreadInfo<'a,Arg,Ret>> {
        let mut threads = Vec::with_capacity(nthreads);
        let mut thread_channels = Vec::with_capacity(nthreads);
        let mut thread_stealers = Vec::with_capacity(nthreads);

        for id in 0..nthreads {
            let (supervisor_channel, worker_port) = channel();
            let thread: WorkerThread<Arg,Ret> = WorkerThread::new(id, worker_port, worker_channel.clone());
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

    fn spawn(self) -> thread::JoinGuard<'a, ()> {
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
                    SupervisorMsg::OutOfWork(id) => self.thread_infos[id].channel.send(WorkerMsg::Steal).unwrap(),
                    SupervisorMsg::Schedule(task) => self.schedule(task),
                    SupervisorMsg::Shutdown => break,
                },
            }
        }
    }

    fn schedule(&mut self, task: Task<Arg, Ret>) {
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
