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

use ::{Task,WorkerMsg,SupervisorMsg};
use ::workerthread::WorkerThread;

use deque::Stealer;

struct ThreadInfo<Arg: 'static + Send, Ret: 'static + Send + Sync> {
    channel: Sender<WorkerMsg<Arg,Ret>>,
    stealer: Stealer<Task<Arg,Ret>>,
}

pub struct PoolSupervisor<Arg: 'static + Send, Ret: 'static + Send + Sync> {
    port: Receiver<SupervisorMsg<Arg,Ret>>,
    thread_infos: Vec<ThreadInfo<Arg,Ret>>,
}

impl<Arg: 'static + Send, Ret: 'static + Send + Sync> PoolSupervisor<Arg,Ret> {
    pub fn new(nthreads: usize) -> Sender<SupervisorMsg<Arg,Ret>> {
        assert!(nthreads > 0);

        let (worker_channel, supervisor_port) = channel();
        let thread_infos = PoolSupervisor::spawn_workers(nthreads, worker_channel.clone());

        PoolSupervisor {
            port: supervisor_port,
            thread_infos: thread_infos,
        }.spawn();

        worker_channel
    }

    fn spawn_workers(nthreads: usize, worker_channel: Sender<SupervisorMsg<Arg,Ret>>) -> Vec<ThreadInfo<Arg,Ret>> {
        let mut threads = Vec::with_capacity(nthreads);
        let mut thread_infos = Vec::with_capacity(nthreads);

        for id in 0..nthreads {
            let (supervisor_channel, worker_port) = channel();
            let thread: WorkerThread<Arg,Ret> = WorkerThread::new(id, worker_port, worker_channel.clone());
            let stealer = thread.get_stealer();

            threads.push(thread);
            thread_infos.push(ThreadInfo {
                channel: supervisor_channel,
                stealer: stealer,
            });
        }

        for (i,thread) in threads.iter_mut().enumerate() {
            for (j,info) in thread_infos.iter().enumerate() {
                if i != j {
                    thread.add_other_stealer(info.stealer.clone());
                }
            }
        }

        for thread in threads {
            thread.spawn();
        }

        thread_infos
    }

    fn spawn(self) {
        let builder = thread::Builder::new().name(format!("fork-join supervisor"));
        let handle = builder.spawn(move|| {
            self.main_loop();
        });
        drop(handle.unwrap()); // Explicitly detach thread to not get compiler warning
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
