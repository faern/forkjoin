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
use std::sync::Arc;
use std::ptr::{Unique,write};
use std::sync::mpsc::{Receiver,Sender};
use std::thread;
use std::mem;
use libc::funcs::posix88::unistd::usleep;

use deque::{BufferPool,Worker,Stealer,Stolen};
use rand::{Rng,XorShiftRng,weak_rng};

use ::{Task,JoinBarrier,TaskResult,ResultReceiver,AlgoStyle,ReduceStyle,Algorithm};
use ::poolsupervisor::SupervisorMsg;

static STEAL_TRIES_UNTIL_BACKOFF: u32 = 30;
static BACKOFF_INC_US: u32 = 10;

pub struct WorkerThread<Arg: Send, Ret: Send + Sync> {
    id: usize,
    started: bool,
    supervisor_port: Receiver<()>,
    supervisor_channel: Sender<SupervisorMsg<Arg, Ret>>,
    deque: Worker<Task<Arg, Ret>>,
    stealer: Stealer<Task<Arg, Ret>>,
    other_stealers: Vec<Stealer<Task<Arg, Ret>>>,
    rng: XorShiftRng,
    sleepers: Arc<AtomicUsize>,
    threadcount: usize,
    stats: ThreadStats,
}

impl<'a, Arg: Send + 'a, Ret: Send + Sync + 'a> WorkerThread<Arg,Ret> {
    pub fn new(id: usize,
            port: Receiver<()>,
            channel: Sender<SupervisorMsg<Arg,Ret>>,
            supervisor_queue: Stealer<Task<Arg, Ret>>,
            sleepers: Arc<AtomicUsize>) -> WorkerThread<Arg,Ret> {
        let pool = BufferPool::new();
        let (worker, stealer) = pool.deque();

        WorkerThread {
            id: id,
            started: false,
            supervisor_port: port,
            supervisor_channel: channel,
            deque: worker,
            stealer: stealer,
            other_stealers: vec![supervisor_queue],
            rng: weak_rng(),
            sleepers: sleepers,
            threadcount: 1, // Myself
            stats: ThreadStats{exec_tasks: 0, steals: 0, steal_fails: 0, sleep_us: 0},
        }
    }

    pub fn get_stealer(&self) -> Stealer<Task<Arg,Ret>> {
        assert!(!self.started);
        self.stealer.clone()
    }

    pub fn add_other_stealer(&mut self, stealer: Stealer<Task<Arg,Ret>>) {
        assert!(!self.started);
        self.other_stealers.push(stealer);
        self.threadcount += 1;
    }

    pub fn spawn(mut self) -> thread::JoinGuard<'a, ()> {
        assert!(!self.started);
        self.started = true;
        let builder = thread::Builder::new().name(format!("fork-join worker {}", self.id+1));
        let joinguard = builder.scoped(move|| {
            self.main_loop();
        });
        match joinguard {
            Ok(j) => j,
            Err(e) => panic!("WorkerThread: unable to start thread: {}", e),
        }
    }

    fn main_loop(mut self) {
        loop {
            match self.supervisor_port.recv() {
                Err(_) => break, // PoolSupervisor has been dropped, lets quit.
                Ok(_) => { // Supervisor instruct to start working
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

    fn process_queue(&mut self) {
        loop {
            match self.deque.pop() {
                Some(task) => self.execute_task(task),
                None => break,
            }
        }
    }

    fn execute_task(&mut self, task: Task<Arg, Ret>) {
        let mut next_task: Option<Task<Arg,Ret>> = Some(task);
        while let Some(task) = next_task {
            if cfg!(feature = "threadstats") {self.stats.exec_tasks += 1;}
            let fun = task.algo.fun;
            match (fun)(task.arg) {
                TaskResult::Done(ret) => {
                    self.handle_done(task.join, ret);
                    next_task = None;
                },
                TaskResult::Fork(args, joinarg) => {
                    next_task = self.handle_fork(task.algo, task.join, args, joinarg);
                }
            }
        }
    }

    fn steal(&mut self) -> Option<Task<Arg,Ret>> {
        if self.other_stealers.len() == 0 {
            None // No one to steal from
        } else {
            let mut backoff_sleep: u32 = BACKOFF_INC_US;
            for try in 0.. {
                match self.try_steal() {
                    Some(task) => return Some(task),
                    None => if try > STEAL_TRIES_UNTIL_BACKOFF {
                        self.sleepers.fetch_add(1, Ordering::SeqCst); // Check number here and set special state if last worker
                        if cfg!(feature = "threadstats") {self.stats.sleep_us += backoff_sleep as usize;}
                        unsafe { usleep(backoff_sleep); }
                        backoff_sleep = backoff_sleep + BACKOFF_INC_US;

                        if self.threadcount == self.sleepers.load(Ordering::SeqCst) {
                            break; // Give up
                        } else {
                            if self.threadcount == self.sleepers.fetch_sub(1, Ordering::SeqCst) {
                                self.sleepers.fetch_add(1, Ordering::SeqCst);
                                break; // Also give up
                            }
                        }
                    },
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
                Stolen::Data(task) => {
                    if cfg!(feature = "threadstats") {self.stats.steals += 1;}
                    return Some(task);
                }
                Stolen::Empty | Stolen::Abort => {
                    if cfg!(feature = "threadstats") {self.stats.steal_fails += 1;}
                    continue;
                }
            }
        }
        None
    }

    fn handle_fork(&self,
        algo: Algorithm<Arg, Ret>,
        join: ResultReceiver<Ret>,
        args: Vec<Arg>,
        joinarg: Option<Ret>) -> Option<Task<Arg,Ret>>
    {
        let len: usize = args.len();
        if len == 0 {
            self.handle_fork_zero(algo, join, joinarg);
            None
        } else {
            match algo.style {
                AlgoStyle::Reduce(reducestyle) => {
                    let (vector, mut ptr_iter) = create_result_vec::<Ret>(len);

                    let mut sub_join = Box::new(JoinBarrier {
                            ret_counter: AtomicUsize::new(len),
                            joinfun: reducestyle,
                            joinarg: joinarg,
                            joinfunarg: vector,
                            parent: join,
                    });

                    let mut args_iter = args.into_iter();
                    let first_task = Task {
                        algo: algo.clone(),
                        arg: args_iter.next().unwrap(),
                        join: ResultReceiver::Join(ptr_iter.next().unwrap(), unsafe{Box::from_raw(&mut *sub_join)}),
                    };
                    loop {
                        match (args_iter.next(), ptr_iter.next()) {
                            (Some(arg), Some(ptr)) => {
                                let forked_task = Task {
                                    algo: algo.clone(),
                                    arg: arg,
                                    join: ResultReceiver::Join(ptr, unsafe{Box::from_raw(&mut *sub_join)}),
                                };
                                self.deque.push(forked_task);
                            },
                            _ => break,
                        }
                    }
                    mem::forget(sub_join); // Don't drop here, last task will take care of that in handle_done
                    Some(first_task)
                },
                AlgoStyle::Search => {
                    for arg in args.into_iter() {
                        let forked_task = Task {
                            algo: algo.clone(),
                            arg: arg,
                            join: join.clone(),
                        };
                        self.deque.push(forked_task);
                    }
                    None
                }
            }
        }
    }

    fn handle_fork_zero(&self, algo: Algorithm<Arg, Ret>, join: ResultReceiver<Ret>, joinarg: Option<Ret>) {
        match algo.style {
            AlgoStyle::Reduce(ref reducestyle) => {
                let joinres = match *reducestyle {
                    ReduceStyle::NoArg(ref joinfun) => (joinfun)(&Vec::new()[..]),
                    ReduceStyle::Arg(ref joinfun) => {
                        let arg = joinarg.unwrap();
                        (joinfun)(&arg, &Vec::new()[..])
                    }
                };
                self.handle_done(join, joinres);
            },
            _ => (),
        }
    }

    fn handle_done(&self, join: ResultReceiver<Ret>, value: Ret) {
        match join {
            ResultReceiver::Join(ptr, joinbarrier) => {
                unsafe { write(*ptr, value); } // Writes without dropping since only null in place
                if joinbarrier.ret_counter.fetch_sub(1, Ordering::SeqCst) == 1 {
                    let joinres = match joinbarrier.joinfun {
                        ReduceStyle::NoArg(ref joinfun) => (joinfun)(&joinbarrier.joinfunarg),
                        ReduceStyle::Arg(ref joinfun) => {
                            let joinarg = match joinbarrier.joinarg.as_ref() {
                                None => panic!("Algorithm has ReduceStyle::Arg, but no extra arg passed"),
                                Some(arg) => arg,
                            };
                            (joinfun)(joinarg, &joinbarrier.joinfunarg)
                        },
                    };
                    self.handle_done(joinbarrier.parent, joinres);
                } else {
                    mem::forget(joinbarrier) // Don't drop if we are not last task
                }
            }
            ResultReceiver::Channel(channel) => {
                channel.lock().unwrap().send(value).unwrap();
            }
        }
    }
}

#[cfg(feature = "threadstats")]
impl<Arg: Send, Ret: Send + Sync> Drop for WorkerThread<Arg, Ret> {
    fn drop(&mut self) {
        println!("WorkerThread[{}] (tasks: {}, steals: {}, failed steals: {}, sleep_us: {})",
            self.id,
            self.stats.exec_tasks,
            self.stats.steals,
            self.stats.steal_fails,
            self.stats.sleep_us);
    }
}

struct ThreadStats {
    pub steals: usize,
    pub steal_fails: usize,
    pub exec_tasks: usize,
    pub sleep_us: usize,
}

fn create_result_vec<Ret>(n: usize) -> (Vec<Ret>, PtrIter<Ret>) {
    let mut rets: Vec<Ret> = Vec::with_capacity(n);
    unsafe {
        rets.set_len(n); // Force it to expand. Values in this will be invalid
        let ptr_0: *mut Ret = rets.get_unchecked_mut(0);
        let ptr_iter = PtrIter {
            ptr_0: ptr_0,
            offset: 0,
        };
        (rets, ptr_iter)
    }
}

struct PtrIter<Ret> {
    ptr_0: *mut Ret,
    offset: isize,
}
impl<Ret> Iterator for PtrIter<Ret> {
    type Item = Unique<Ret>;

    fn next(&mut self) -> Option<Self::Item> {
        let ptr = unsafe { Unique::new(self.ptr_0.offset(self.offset)) };
        self.offset += 1;
        Some(ptr)
    }
}
