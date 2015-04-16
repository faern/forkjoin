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


use std::sync::atomic::{AtomicUsize,Ordering};
use std::sync::{Arc};
use std::ptr::{Unique,write};
use std::sync::mpsc::{Receiver,Sender};
use std::thread;

use deque::{BufferPool,Worker,Stealer,Stolen};
use rand::{Rng,XorShiftRng,weak_rng};

use ::{Task,JoinBarrier,TaskResult,Fork,ResultReceiver,WorkerMsg,AlgoStyle};
use ::poolsupervisor::SupervisorMsg;

static STEAL_TRIES: usize = 5;

pub struct WorkerThread<Arg: Send, Ret: Send + Sync> {
    id: usize,
    started: bool,
    supervisor_port: Receiver<WorkerMsg<Arg,Ret>>,
    supervisor_channel: Sender<SupervisorMsg<Arg,Ret>>,
    deque: Worker<Task<Arg,Ret>>,
    stealer: Stealer<Task<Arg,Ret>>,
    other_stealers: Vec<Stealer<Task<Arg,Ret>>>,
    rng: XorShiftRng,
}

impl<'a, Arg: Send + 'a, Ret: Send + Sync + 'a> WorkerThread<Arg,Ret> {
    pub fn new(id: usize,
            port: Receiver<WorkerMsg<Arg,Ret>>,
            channel: Sender<SupervisorMsg<Arg,Ret>>) -> WorkerThread<Arg,Ret> {
        let pool = BufferPool::new();
        let (worker, stealer) = pool.deque();

        WorkerThread {
            id: id,
            started: false,
            supervisor_port: port,
            supervisor_channel: channel,
            deque: worker,
            stealer: stealer,
            other_stealers: vec![],
            rng: weak_rng(),
        }
    }

    pub fn get_stealer(&self) -> Stealer<Task<Arg,Ret>> {
        assert!(!self.started);
        self.stealer.clone()
    }

    pub fn add_other_stealer(&mut self, stealer: Stealer<Task<Arg,Ret>>) {
        assert!(!self.started);
        self.other_stealers.push(stealer);
    }

    pub fn spawn(mut self) -> thread::JoinGuard<'a, ()> {
        assert!(!self.started);
        self.started = true;
        let builder = thread::Builder::new().name(format!("fork-join worker {}", self.id+1));
        let joinguard = builder.scoped(move|| {
            self.main_loop();
        });
        joinguard.unwrap()
    }

    fn main_loop(mut self) {
        loop {
            match self.supervisor_port.recv() {
                Err(_) => break, // PoolSupervisor has been dropped, lets quit.
                Ok(msg) => {
                    match msg {
                        WorkerMsg::Schedule(task) => self.execute_task(task),
                        WorkerMsg::Steal => (), // Do nothing, it will come to steal further down
                    }
                    loop {
                        self.process_queue();
                        match self.steal() {
                            Some(task) => self.execute_task(task),
                            None => break, // Give up for now
                        }
                    }
                }
            }
            if self.supervisor_channel.send(SupervisorMsg::OutOfWork(self.id)).is_err() {
                break; // Supervisor shut down, so we also shut down
            }
        }
    }

    fn process_queue(&self) {
        loop {
            match self.deque.pop() {
                Some(task) => self.execute_task(task),
                None => break,
            }
        }
    }

    fn execute_task(&self, task: Task<Arg, Ret>) {
        match (task.fun)(task.arg) {
            TaskResult::Done(ret) => {
                self.handle_join(&task.join, ret);
            },
            TaskResult::Fork(fork) => {
                self.handle_fork(fork, task.join);
            }
        }
    }

    fn steal(&mut self) -> Option<Task<Arg,Ret>> {
        if self.other_stealers.len() == 0 {
            None
        } else {
            for try in 0..STEAL_TRIES {
                match self.try_steal() {
                    Some(task) => return Some(task),
                    None => if try > 0 { thread::sleep_ms(1); } else { thread::yield_now(); },
                }
            }
            None
        }
    }

    /// Try to steal tasks from the other workers.
    /// Starts at a random worker and tries every worker until a task is stolen or
    /// every worker has been tried once.
    fn try_steal(&mut self) -> Option<Task<Arg,Ret>> {
        let len = self.other_stealers.len();
        let start_victim = self.rng.gen_range(0, len);
        for offset in 0..len {
            match self.other_stealers[(start_victim + offset) % len].steal() {
                Stolen::Data(task) => return Some(task),
                Stolen::Empty | Stolen::Abort => continue,
            }
        }
        None
    }

    fn handle_fork(&self, fork: Fork<Arg, Ret>, parent_join: ResultReceiver<Ret>) {
        let len: usize = fork.args.len();
        let mut resultreceivers = vec![];
        match fork.join {
            AlgoStyle::Summa(joinfun) => {
                if len == 0 {
                    let joinres = (joinfun)(&Vec::new()[..]);
                    self.handle_join(&parent_join, joinres);
                } else {
                    let (rets, rets_ptr) = create_result_vec::<Ret>(len);

                    let join_arc = Arc::new(JoinBarrier {
                        ret_counter: AtomicUsize::new(len),
                        joinfun: joinfun,
                        joinfunarg: rets,
                        parent: parent_join,
                    });

                    for ptr in rets_ptr.into_iter() {
                        resultreceivers.push(ResultReceiver::Join(ptr, join_arc.clone()));
                    }
                }
            },
            AlgoStyle::Search => {
                for _ in 0..len {
                    resultreceivers.push(parent_join.clone());
                }
            }
        }

        // Will loop until one vec run out. So if 0 forks, will not run a single iteration
        for (arg,resultreceiver) in fork.args.into_iter().zip(resultreceivers.into_iter()) {
            let task = Task {
                fun: fork.fun,
                arg: arg,
                join: resultreceiver,
            };
            self.deque.push(task);
        }
    }

    fn handle_join(&self, join: &ResultReceiver<Ret>, value: Ret) {
        match *join {
            ResultReceiver::Join(ref ptr, ref join) => {
                unsafe { write(**ptr, value); } // Writes without dropping since only null in place
                if join.ret_counter.fetch_sub(1, Ordering::SeqCst) == 1 {
                    let joinres = (join.joinfun)(&join.joinfunarg);
                    self.handle_join(&join.parent, joinres);
                }
            }
            ResultReceiver::Channel(ref channel) => {
                match channel.lock().unwrap().send(value) {
                    Err(..) => println!("Computation owner don't listen for results anymore"),
                    _ => (),
                }
            }
        }
    }
}

impl<Arg: Send, Ret: Send + Sync> Drop for WorkerThread<Arg, Ret> {
    fn drop(&mut self) {
        println!("Dropping WorkerThread");
    }
}

fn create_result_vec<Ret>(n: usize) -> (Vec<Ret>, Vec<Unique<Ret>>) {
    let mut rets: Vec<Ret> = Vec::with_capacity(n);
    let mut rets_ptr: Vec<Unique<Ret>> = Vec::with_capacity(n);
    unsafe {
        rets.set_len(n); // Force it to expand. Values in this will be invalid
        let ptr_0: *mut Ret = rets.get_unchecked_mut(0);
        for i in 0..(n as isize) {
            rets_ptr.push(Unique::new(ptr_0.offset(i)));
        }
    }
    (rets, rets_ptr)
}
