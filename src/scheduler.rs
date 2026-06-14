//! Multi-model job scheduling and resource allocation
//!
//! Implements a priority-based job scheduler with:
//! - BinaryHeap-based max-heap priority queue
//! - ResourceAllocator with atomic check-and-reserve
//! - Job preemption for high-priority (>=8) jobs
//! - Concurrent job support with configurable resource limits
//!
//! Implements Requirement 26.

use chrono::{DateTime, Utc};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::types::JobState;

// ── ResourceAllocator ─────────────────────────────────────────────────────────

/// Tracks resource limits and current allocations.
/// All `try_allocate` / `release` operations are atomic (single-threaded value semantics).
#[derive(Debug, Clone)]
pub struct ResourceAllocator {
    /// Maximum CPU percentage available to all jobs combined
    pub max_cpu_percent: f32,
    /// Maximum RAM in GB available to all jobs combined
    pub max_ram_gb: f32,
    /// Maximum GPU memory in GB (None if no GPU)
    pub max_gpu_memory_gb: Option<f32>,
    /// Currently allocated CPU percentage
    pub allocated_cpu: f32,
    /// Currently allocated RAM in GB
    pub allocated_ram: f32,
    /// Currently allocated GPU memory in GB
    pub allocated_gpu: f32,
}

impl ResourceAllocator {
    /// Create a new ResourceAllocator with the given limits.
    pub fn new(max_cpu: f32, max_ram: f32, max_gpu: Option<f32>) -> Self {
        Self {
            max_cpu_percent: max_cpu,
            max_ram_gb: max_ram,
            max_gpu_memory_gb: max_gpu,
            allocated_cpu: 0.0,
            allocated_ram: 0.0,
            allocated_gpu: 0.0,
        }
    }

    /// Remaining CPU percentage available for allocation.
    pub fn available_cpu(&self) -> f32 {
        (self.max_cpu_percent - self.allocated_cpu).max(0.0)
    }

    /// Remaining RAM in GB available for allocation.
    pub fn available_ram(&self) -> f32 {
        (self.max_ram_gb - self.allocated_ram).max(0.0)
    }

    /// Atomically check whether `required_cpu` and `required_ram` can be
    /// satisfied and, if so, reserve them.  Returns `Some(())` on success,
    /// `None` if either resource would be exceeded.
    pub fn try_allocate(&mut self, required_cpu: f32, required_ram: f32) -> Option<()> {
        if self.allocated_cpu + required_cpu > self.max_cpu_percent + f32::EPSILON {
            return None;
        }
        if self.allocated_ram + required_ram > self.max_ram_gb + f32::EPSILON {
            return None;
        }
        self.allocated_cpu += required_cpu;
        self.allocated_ram += required_ram;
        Some(())
    }

    /// Release previously allocated resources back to the pool.
    pub fn release(&mut self, cpu: f32, ram: f32) {
        self.allocated_cpu = (self.allocated_cpu - cpu).max(0.0);
        self.allocated_ram = (self.allocated_ram - ram).max(0.0);
    }
}

// ── QueuedJob ─────────────────────────────────────────────────────────────────

/// A job waiting in the priority queue.
/// `Ord` is implemented so that `BinaryHeap` acts as a max-heap on `priority`.
#[derive(Debug, Clone)]
pub struct QueuedJob {
    /// Unique job identifier
    pub job_id: String,
    /// Model this job belongs to
    pub model_id: String,
    /// Scheduling priority (0–255, higher = more important)
    pub priority: u8,
    /// Federated learning epoch number
    pub epoch_number: u64,
    /// When this job was enqueued (used as tie-breaker: earlier = higher)
    pub queued_at: DateTime<Utc>,
    /// Required CPU percentage
    pub required_cpu: f32,
    /// Required RAM in GB
    pub required_ram: f32,
}

// QueuedJob equality is based purely on job_id (the unique key).
// This satisfies BinaryHeap's Eq requirement without requiring f32: Eq.
impl PartialEq for QueuedJob {
    fn eq(&self, other: &Self) -> bool {
        self.job_id == other.job_id
    }
}
impl Eq for QueuedJob {}

impl Ord for QueuedJob {
    fn cmp(&self, other: &Self) -> Ordering {
        // Primary: higher priority first
        self.priority
            .cmp(&other.priority)
            // Secondary: earlier enqueue time first (FIFO within same priority)
            .then_with(|| other.queued_at.cmp(&self.queued_at))
    }
}

impl PartialOrd for QueuedJob {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// ── RunningJob ────────────────────────────────────────────────────────────────

/// A job that has been dispatched and is consuming resources.
#[derive(Debug, Clone)]
pub struct RunningJob {
    /// Unique job identifier
    pub job_id: String,
    /// Model this job belongs to
    pub model_id: String,
    /// Scheduling priority
    pub priority: u8,
    /// Federated learning epoch number
    pub epoch_number: u64,
    /// Current lifecycle state
    pub state: JobState,
    /// CPU percentage reserved for this job
    pub allocated_cpu: f32,
    /// RAM in GB reserved for this job
    pub allocated_ram: f32,
}

// ── JobScheduler ──────────────────────────────────────────────────────────────

/// Priority-based job scheduler with resource tracking and preemption support.
///
/// # Scheduling semantics
/// * Jobs are enqueued into a `BinaryHeap` (max-heap on priority).
/// * `try_schedule` pops the highest-priority job and attempts resource
///   allocation. If resources are exhausted **and** the candidate has
///   priority ≥ 8, it will preempt the lowest-priority running job before
///   retrying.
/// * Preempted jobs are re-enqueued with their original priority.
pub struct JobScheduler {
    /// Currently executing jobs, keyed by `job_id`
    active_jobs: HashMap<String, RunningJob>,
    /// Pending jobs ordered by priority (max-heap)
    job_queue: BinaryHeap<QueuedJob>,
    /// Resource accounting
    resource_allocator: ResourceAllocator,
    /// Hard limit on simultaneous running jobs
    max_concurrent_jobs: usize,
}

impl JobScheduler {
    /// Create a new scheduler.
    ///
    /// # Parameters
    /// * `max_cpu`        – total CPU % budget for all running jobs
    /// * `max_ram`        – total RAM budget (GB) for all running jobs
    /// * `max_concurrent` – maximum number of simultaneously running jobs
    pub fn new(max_cpu: f32, max_ram: f32, max_concurrent: usize) -> Self {
        Self {
            active_jobs: HashMap::new(),
            job_queue: BinaryHeap::new(),
            resource_allocator: ResourceAllocator::new(max_cpu, max_ram, None),
            max_concurrent_jobs: max_concurrent,
        }
    }

    /// Add a job to the priority queue.
    ///
    /// Default resource estimates (20% CPU, 2 GB RAM) are overridden by the
    /// caller-supplied `required_cpu` / `required_ram` values.
    #[allow(clippy::too_many_arguments)]
    pub fn enqueue(
        &mut self,
        job_id: impl Into<String>,
        model_id: impl Into<String>,
        priority: u8,
        epoch_number: u64,
        required_cpu: f32,
        required_ram: f32,
    ) {
        self.job_queue.push(QueuedJob {
            job_id: job_id.into(),
            model_id: model_id.into(),
            priority,
            epoch_number,
            queued_at: Utc::now(),
            required_cpu,
            required_ram,
        });
    }

    /// Attempt to start the highest-priority queued job.
    ///
    /// Returns the `RunningJob` when a job is successfully started.
    /// Returns `None` when:
    /// * the queue is empty,
    /// * the concurrent-job limit has been reached, or
    /// * there are insufficient resources (and the candidate is not eligible
    ///   for preemption, or there is nothing to preempt).
    pub fn try_schedule(&mut self) -> Option<RunningJob> {
        // Concurrent-job ceiling
        if self.active_jobs.len() >= self.max_concurrent_jobs {
            return None;
        }

        // Peek at the highest-priority candidate without removing it yet
        let candidate = self.job_queue.peek()?.clone();

        // Try to allocate resources directly
        if self
            .resource_allocator
            .try_allocate(candidate.required_cpu, candidate.required_ram)
            .is_some()
        {
            // Allocation succeeded – now pop and start the job
            self.job_queue.pop();
            let running = RunningJob {
                job_id: candidate.job_id,
                model_id: candidate.model_id,
                priority: candidate.priority,
                epoch_number: candidate.epoch_number,
                state: JobState::Running,
                allocated_cpu: candidate.required_cpu,
                allocated_ram: candidate.required_ram,
            };
            self.active_jobs.insert(running.job_id.clone(), running.clone());
            return Some(running);
        }

        // Allocation failed. Attempt preemption only for high-priority jobs.
        if candidate.priority >= 8 && !self.active_jobs.is_empty() {
            // Only preempt if there is a running job with strictly lower priority
            let victim_priority = self
                .active_jobs
                .values()
                .map(|j| j.priority)
                .min()
                .unwrap_or(u8::MAX);

            if candidate.priority <= victim_priority {
                // Nothing worth preempting (candidate is not strictly higher)
                return None;
            }

            self.preempt_lowest_priority()?;

            // Retry allocation after freeing resources
            if self
                .resource_allocator
                .try_allocate(candidate.required_cpu, candidate.required_ram)
                .is_some()
            {
                self.job_queue.pop();
                let running = RunningJob {
                    job_id: candidate.job_id,
                    model_id: candidate.model_id,
                    priority: candidate.priority,
                    epoch_number: candidate.epoch_number,
                    state: JobState::Running,
                    allocated_cpu: candidate.required_cpu,
                    allocated_ram: candidate.required_ram,
                };
                self.active_jobs.insert(running.job_id.clone(), running.clone());
                return Some(running);
            }
        }

        None
    }

    /// Mark a job as completed, release its resources, and return the job record.
    pub fn complete_job(&mut self, job_id: &str) -> Option<RunningJob> {
        let mut job = self.active_jobs.remove(job_id)?;
        self.resource_allocator
            .release(job.allocated_cpu, job.allocated_ram);
        job.state = JobState::Completed;
        Some(job)
    }

    /// Mark a job as failed, release its resources, and return the job record.
    ///
    /// The `reason` parameter is accepted for API consistency; callers may log
    /// it externally.
    pub fn fail_job(&mut self, job_id: &str, _reason: &str) -> Option<RunningJob> {
        let mut job = self.active_jobs.remove(job_id)?;
        self.resource_allocator
            .release(job.allocated_cpu, job.allocated_ram);
        job.state = JobState::Failed;
        Some(job)
    }

    /// Pause and re-enqueue the lowest-priority active job, releasing its resources.
    ///
    /// Returns the paused `RunningJob`, or `None` if there are no active jobs.
    pub fn preempt_lowest_priority(&mut self) -> Option<RunningJob> {
        // Find the job_id of the lowest-priority running job
        let victim_id = self
            .active_jobs
            .values()
            .min_by_key(|j| j.priority)
            .map(|j| j.job_id.clone())?;

        let mut victim = self.active_jobs.remove(&victim_id)?;
        self.resource_allocator
            .release(victim.allocated_cpu, victim.allocated_ram);
        victim.state = JobState::Paused;

        // Re-enqueue the paused job so it can be rescheduled later
        self.job_queue.push(QueuedJob {
            job_id: victim.job_id.clone(),
            model_id: victim.model_id.clone(),
            priority: victim.priority,
            epoch_number: victim.epoch_number,
            queued_at: Utc::now(),
            required_cpu: victim.allocated_cpu,
            required_ram: victim.allocated_ram,
        });

        Some(victim)
    }

    /// Number of currently running jobs.
    pub fn active_count(&self) -> usize {
        self.active_jobs.len()
    }

    /// Number of jobs waiting in the queue.
    pub fn queued_count(&self) -> usize {
        self.job_queue.len()
    }

    /// Snapshot of all currently running jobs.
    pub fn get_active_jobs(&self) -> Vec<&RunningJob> {
        self.active_jobs.values().collect()
    }

    /// Expose the resource allocator for inspection (e.g., in tests).
    pub fn resource_allocator(&self) -> &ResourceAllocator {
        &self.resource_allocator
    }
}

// ── Unit Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: scheduler with plenty of headroom (100% CPU, 100 GB RAM, 10 slots)
    fn large_scheduler() -> JobScheduler {
        JobScheduler::new(100.0, 100.0, 10)
    }

    // ── 1. Enqueue and schedule ───────────────────────────────────────────────

    /// Enqueuing a single job then calling try_schedule returns that job.
    #[test]
    fn test_enqueue_and_schedule() {
        let mut sched = large_scheduler();
        sched.enqueue("job-1", "model-a", 5, 1, 20.0, 2.0);

        let job = sched.try_schedule().expect("should schedule the job");
        assert_eq!(job.job_id, "job-1");
        assert_eq!(job.model_id, "model-a");
        assert_eq!(job.priority, 5);
        assert_eq!(job.state, JobState::Running);
        assert_eq!(sched.active_count(), 1);
        assert_eq!(sched.queued_count(), 0);
    }

    // ── 2. Priority ordering ──────────────────────────────────────────────────

    /// When multiple jobs are queued, higher-priority job is scheduled first.
    #[test]
    fn test_priority_ordering() {
        let mut sched = large_scheduler();
        sched.enqueue("job-low", "model-a", 3, 1, 20.0, 2.0);
        sched.enqueue("job-high", "model-a", 9, 1, 20.0, 2.0);
        sched.enqueue("job-mid", "model-a", 6, 1, 20.0, 2.0);

        let first = sched.try_schedule().expect("first schedule");
        assert_eq!(first.job_id, "job-high", "highest priority should run first");

        let second = sched.try_schedule().expect("second schedule");
        assert_eq!(second.job_id, "job-mid");

        let third = sched.try_schedule().expect("third schedule");
        assert_eq!(third.job_id, "job-low");
    }

    // ── 3. Resource limits block scheduling ──────────────────────────────────

    /// When resources are exhausted (CPU) and no preemption threshold reached,
    /// try_schedule returns None.
    #[test]
    fn test_resource_limits_block_scheduling() {
        // Only 30% CPU available total; each job needs 20%
        let mut sched = JobScheduler::new(30.0, 100.0, 10);
        sched.enqueue("job-1", "model-a", 5, 1, 20.0, 2.0);
        sched.enqueue("job-2", "model-a", 4, 1, 20.0, 2.0); // won't fit

        let first = sched.try_schedule().expect("first job fits");
        assert_eq!(first.job_id, "job-1");

        // Second job (priority 4, below preemption threshold 8) should be blocked
        let second = sched.try_schedule();
        assert!(second.is_none(), "second job should be blocked by resource limits");
        assert_eq!(sched.queued_count(), 1, "blocked job stays in queue");
    }

    // ── 4. Preemption of lower-priority job ──────────────────────────────────

    /// A high-priority (>=8) job preempts the lowest-priority running job.
    #[test]
    fn test_preemption_of_lower_priority() {
        // Tight resource limit: only enough for one 20% CPU job
        let mut sched = JobScheduler::new(30.0, 100.0, 10);

        // Schedule a low-priority job first
        sched.enqueue("job-low", "model-a", 3, 1, 20.0, 2.0);
        sched.try_schedule().expect("low-priority job starts");
        assert_eq!(sched.active_count(), 1);

        // Enqueue a high-priority job that needs the same resources
        sched.enqueue("job-high", "model-a", 9, 2, 20.0, 2.0);

        let high_job = sched.try_schedule().expect("high-priority job should preempt");
        assert_eq!(high_job.job_id, "job-high");
        assert_eq!(high_job.state, JobState::Running);

        // The preempted job must be back in the queue (paused → re-enqueued)
        assert_eq!(sched.queued_count(), 1, "preempted job should be re-enqueued");
        assert_eq!(sched.active_count(), 1, "only the high-priority job runs now");
    }

    // ── 5. Completing a job releases resources ───────────────────────────────

    /// After complete_job, the freed resources allow the next queued job to run.
    #[test]
    fn test_complete_job_releases_resources() {
        let mut sched = JobScheduler::new(30.0, 100.0, 10);

        sched.enqueue("job-1", "model-a", 5, 1, 20.0, 2.0);
        sched.enqueue("job-2", "model-a", 4, 1, 20.0, 2.0);

        let j1 = sched.try_schedule().expect("job-1 starts");
        assert!(sched.try_schedule().is_none(), "no room for job-2 yet");

        let completed = sched.complete_job(&j1.job_id).expect("complete job-1");
        assert_eq!(completed.state, JobState::Completed);

        // Resources freed; job-2 should now run
        let j2 = sched.try_schedule().expect("job-2 should now schedule");
        assert_eq!(j2.job_id, "job-2");

        // Verify resource accounting is correct
        assert!(
            (sched.resource_allocator().allocated_cpu - 20.0).abs() < f32::EPSILON,
            "only job-2's CPU should be allocated"
        );
    }

    // ── 6. Multiple concurrent jobs ───────────────────────────────────────────

    /// When resources allow, two jobs run simultaneously.
    #[test]
    fn test_multiple_concurrent_jobs() {
        let mut sched = JobScheduler::new(100.0, 20.0, 10);

        sched.enqueue("job-1", "model-a", 5, 1, 20.0, 2.0);
        sched.enqueue("job-2", "model-b", 5, 1, 20.0, 2.0);

        sched.try_schedule().expect("job-1 starts");
        sched.try_schedule().expect("job-2 starts concurrently");

        assert_eq!(sched.active_count(), 2);
        assert_eq!(sched.queued_count(), 0);

        // Both jobs' resources are counted
        let alloc = sched.resource_allocator();
        assert!((alloc.allocated_cpu - 40.0).abs() < f32::EPSILON);
        assert!((alloc.allocated_ram - 4.0).abs() < f32::EPSILON);
    }

    // ── 7. fail_job ───────────────────────────────────────────────────────────

    /// fail_job marks the job failed and releases resources.
    #[test]
    fn test_fail_job() {
        let mut sched = large_scheduler();
        sched.enqueue("job-1", "model-a", 5, 1, 20.0, 2.0);
        let j = sched.try_schedule().unwrap();

        let failed = sched.fail_job(&j.job_id, "timeout").expect("fail job-1");
        assert_eq!(failed.state, JobState::Failed);
        assert_eq!(sched.active_count(), 0);
        assert!(sched.resource_allocator().allocated_cpu < f32::EPSILON);
    }

    // ── 8. Concurrent-job ceiling ────────────────────────────────────────────

    /// try_schedule returns None when max_concurrent_jobs is reached.
    #[test]
    fn test_concurrent_job_ceiling() {
        let mut sched = JobScheduler::new(100.0, 100.0, 2);
        sched.enqueue("job-1", "model-a", 5, 1, 10.0, 1.0);
        sched.enqueue("job-2", "model-a", 5, 1, 10.0, 1.0);
        sched.enqueue("job-3", "model-a", 5, 1, 10.0, 1.0);

        sched.try_schedule().expect("job-1");
        sched.try_schedule().expect("job-2");

        assert!(
            sched.try_schedule().is_none(),
            "ceiling of 2 concurrent jobs must be enforced"
        );
    }
}

// ── Property-Based Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod prop_tests {
    use super::*;
    use proptest::prelude::*;

    /// Constrained strategy for a single job's parameters.
    fn job_strategy() -> impl Strategy<Value = (u8, f32, f32)> {
        (
            1u8..=10u8,          // priority
            1.0f32..=20.0f32,    // required_cpu %
            0.5f32..=2.0f32,     // required_ram GB
        )
    }

    // ── Property 35: Job Scheduling Priority Ordering ─────────────────────────
    //
    // Validates: Requirements 26.5, 26.8
    //
    // Given N jobs with distinct priorities, when scheduled one at a time with
    // enough resources, jobs always emerge in non-increasing priority order.

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 26.5, 26.8**
        ///
        /// The first job scheduled always has the maximum priority among all
        /// enqueued jobs.
        #[test]
        fn prop_highest_priority_scheduled_first(
            jobs in prop::collection::vec(job_strategy(), 2..=8usize)
        ) {
            // Use a scheduler with ample resources so only priority determines order
            let mut sched = JobScheduler::new(1000.0, 1000.0, 20);

            for (idx, (priority, cpu, ram)) in jobs.iter().enumerate() {
                sched.enqueue(
                    format!("job-{idx}"),
                    "model-test",
                    *priority,
                    idx as u64,
                    *cpu,
                    *ram,
                );
            }

            let max_priority = jobs.iter().map(|(p, _, _)| *p).max().unwrap();

            let first = sched.try_schedule().expect("at least one job should schedule");
            prop_assert_eq!(
                first.priority,
                max_priority,
                "first scheduled job must have the highest priority"
            );

            // Verify non-increasing order for remaining jobs
            let mut last_priority = first.priority;
            let remaining = jobs.len(); // bounded iteration
            for _ in 0..remaining {
                let next = match sched.try_schedule() {
                    Some(j) => j,
                    None => break,
                };
                prop_assert!(
                    next.priority <= last_priority,
                    "jobs must be scheduled in non-increasing priority order: {} > {}",
                    next.priority,
                    last_priority
                );
                last_priority = next.priority;
            }
        }
    }

    // ── Property 36: Resource Allocation Limits ───────────────────────────────
    //
    // Validates: Requirements 26.5, 26.9
    //
    // After scheduling any number of jobs, total allocated CPU and RAM never
    // exceed the configured maximums.

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 26.5, 26.9**
        ///
        /// Allocated resources never exceed the configured maximums after any
        /// sequence of enqueue + try_schedule operations.
        #[test]
        fn prop_allocated_never_exceeds_max(
            max_cpu in 10.0f32..=200.0f32,
            max_ram in 1.0f32..=64.0f32,
            jobs in prop::collection::vec(job_strategy(), 1..=10usize),
        ) {
            let max_concurrent = 8usize;
            let mut sched = JobScheduler::new(max_cpu, max_ram, max_concurrent);

            for (idx, (priority, cpu, ram)) in jobs.iter().enumerate() {
                sched.enqueue(
                    format!("job-{idx}"),
                    "model-test",
                    *priority,
                    idx as u64,
                    *cpu,
                    *ram,
                );
            }

            // Schedule as many as possible — bounded to prevent infinite loops
            // (at most jobs.len() schedules can succeed since no new jobs are added)
            for _ in 0..jobs.len() {
                if sched.try_schedule().is_none() {
                    break;
                }
            }

            let alloc = sched.resource_allocator();
            prop_assert!(
                alloc.allocated_cpu <= alloc.max_cpu_percent + f32::EPSILON,
                "allocated CPU {} must not exceed max {}",
                alloc.allocated_cpu,
                alloc.max_cpu_percent
            );
            prop_assert!(
                alloc.allocated_ram <= alloc.max_ram_gb + f32::EPSILON,
                "allocated RAM {} must not exceed max {}",
                alloc.allocated_ram,
                alloc.max_ram_gb
            );
        }
    }

    // ── Property 37: Job Preemption ───────────────────────────────────────────
    //
    // Validates: Requirements 26.8, 26.10, 26.11
    //
    // After preempting a job, the preempted job moves to the queue and its
    // resources are released.

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 26.8, 26.10, 26.11**
        ///
        /// After `preempt_lowest_priority` is called:
        /// * the previously active job count decreases by exactly one,
        /// * the queue count increases by exactly one, and
        /// * allocated resources decrease (reflect the released job).
        #[test]
        fn prop_preemption_frees_resources(
            jobs in prop::collection::vec(job_strategy(), 1..=5usize),
        ) {
            let mut sched = JobScheduler::new(1000.0, 1000.0, 20);

            // Enqueue and schedule all jobs so there are active jobs to preempt
            for (idx, (priority, cpu, ram)) in jobs.iter().enumerate() {
                sched.enqueue(
                    format!("job-{idx}"),
                    "model-test",
                    *priority,
                    idx as u64,
                    *cpu,
                    *ram,
                );
            }
            // Bounded scheduling loop
            for _ in 0..jobs.len() {
                if sched.try_schedule().is_none() {
                    break;
                }
            }

            // Only test preemption when there is at least one active job
            if sched.active_count() == 0 {
                return Ok(());
            }

            let active_before = sched.active_count();
            let queued_before = sched.queued_count();
            let cpu_before = sched.resource_allocator().allocated_cpu;
            let ram_before = sched.resource_allocator().allocated_ram;

            let preempted = sched
                .preempt_lowest_priority()
                .expect("should preempt when active jobs exist");

            prop_assert_eq!(
                sched.active_count(),
                active_before - 1,
                "active count should decrease by 1"
            );
            prop_assert_eq!(
                sched.queued_count(),
                queued_before + 1,
                "queue count should increase by 1 (re-enqueued)"
            );
            prop_assert!(
                sched.resource_allocator().allocated_cpu < cpu_before + f32::EPSILON,
                "allocated CPU should decrease after preemption"
            );
            prop_assert!(
                sched.resource_allocator().allocated_ram < ram_before + f32::EPSILON,
                "allocated RAM should decrease after preemption"
            );
            prop_assert_eq!(
                preempted.state,
                JobState::Paused,
                "preempted job state must be Paused"
            );
        }
    }
}
