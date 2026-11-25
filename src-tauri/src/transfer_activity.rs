use serde::Serialize;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct ActivityJob {
    pub id: String,
    pub upload: bool,
    pub total: u64,
    pub completed: u64,
    pub status: String,
}

struct Sample {
    completed: u64,
    speed: f64,
    at: Instant,
}

struct Entry {
    job: ActivityJob,
    sample: Option<Sample>,
    hashing: bool,
}

#[derive(Default)]
pub struct TransferActivity {
    jobs: HashMap<String, Entry>,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectionOverview {
    pub total_jobs: u64,
    pub completed_jobs: u64,
    pub total_bytes: u64,
    pub completed_bytes: u64,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferOverview {
    pub upload: DirectionOverview,
    pub download: DirectionOverview,
    pub total_jobs: u64,
    pub completed_jobs: u64,
    pub active_jobs: u64,
    pub queued_jobs: u64,
    pub paused_jobs: u64,
    pub failed_jobs: u64,
    pub hashing_jobs: u64,
    pub total_bytes: u64,
    pub completed_bytes: u64,
    pub upload_speed: f64,
    pub download_speed: f64,
    pub upload_remaining: u64,
    pub download_remaining: u64,
}

impl TransferOverview {
    pub fn byte_percent(&self) -> f64 {
        if self.total_bytes == 0 {
            self.job_percent()
        } else {
            (self.completed_bytes as f64 / self.total_bytes as f64 * 100.0).clamp(0.0, 100.0)
        }
    }
    pub fn job_percent(&self) -> f64 {
        if self.total_jobs == 0 {
            0.0
        } else {
            self.completed_jobs as f64 / self.total_jobs as f64 * 100.0
        }
    }
    pub fn speed_text(&self) -> String {
        format!(
            "上传 {}/s · 下载 {}/s",
            bytes(self.upload_speed.max(0.0) as u64),
            bytes(self.download_speed.max(0.0) as u64)
        )
    }
    pub fn progress_text(&self) -> String {
        format!(
            "任务 {}/{}（{:.1}%）· 运行 {} · 等待 {} · 暂停 {} · 失败 {}",
            self.completed_jobs,
            self.total_jobs,
            self.job_percent(),
            self.active_jobs,
            self.queued_jobs,
            self.paused_jobs,
            self.failed_jobs
        )
    }
    pub fn size_text(&self) -> String {
        format!(
            "大小 {} / {}（{:.1}%）",
            bytes(self.completed_bytes),
            bytes(self.total_bytes),
            self.byte_percent()
        )
    }
}

pub fn bytes(value: u64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut amount = value as f64;
    let mut unit = 0;
    while amount >= 1024.0 && unit < units.len() - 1 {
        amount /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value} B")
    } else {
        format!("{amount:.1} {}", units[unit])
    }
}

fn terminal(status: &str) -> bool {
    matches!(status, "completed" | "cancelled")
}

impl TransferActivity {
    pub fn clear(&mut self) {
        self.jobs.clear();
    }

    // Keep completed members until the next batch, so progress does not jump
    // backwards whenever a file finishes. Previously completed history is not enrolled.
    pub fn observe(&mut self, jobs: Vec<ActivityJob>) {
        if !jobs.is_empty() && self.jobs.values().all(|entry| terminal(&entry.job.status)) {
            self.jobs.clear();
        }
        for job in jobs {
            match self.jobs.get_mut(&job.id) {
                Some(entry) => {
                    if job.status != "running" {
                        entry.sample = None;
                        entry.hashing = false;
                    }
                    entry.job = job;
                }
                None => {
                    self.jobs.insert(
                        job.id.clone(),
                        Entry {
                            job,
                            sample: None,
                            hashing: false,
                        },
                    );
                }
            }
        }
    }

    pub fn set_status(&mut self, id: &str, status: &str) {
        if let Some(entry) = self.jobs.get_mut(id) {
            entry.job.status = status.into();
            if status == "completed" {
                entry.job.completed = entry.job.total;
            }
            entry.sample = None;
            entry.hashing = false;
        }
    }

    pub fn progress(&mut self, id: &str, completed: u64, total: u64, speed: f64, phase: &str) {
        if let Some(entry) = self.jobs.get_mut(id) {
            entry.hashing = matches!(phase,"hashing"|"encrypting"|"decrypting"|"publishing");
            if entry.hashing {
                entry.sample = None;
            } else {
                entry.job.total = total;
                entry.sample = Some(Sample {
                    completed: completed.min(total),
                    speed: if speed.is_finite() {
                        speed.max(0.0)
                    } else {
                        0.0
                    },
                    at: Instant::now(),
                });
            }
        }
    }

    pub fn overview(&self) -> TransferOverview {
        let mut value = TransferOverview::default();
        for entry in self
            .jobs
            .values()
            .filter(|entry| entry.job.status != "cancelled")
        {
            let job = &entry.job;
            value.total_jobs += 1;
            value.total_bytes = value.total_bytes.saturating_add(job.total);
            let sample = entry.sample.as_ref().filter(|sample| {
                job.status == "running" && sample.at.elapsed() < Duration::from_secs(4)
            });
            let done = if job.status == "completed" {
                job.total
            } else {
                sample
                    .map_or(job.completed, |sample| sample.completed.max(job.completed))
                    .min(job.total)
            };
            value.completed_bytes = value.completed_bytes.saturating_add(done);
            let direction = if job.upload {
                &mut value.upload
            } else {
                &mut value.download
            };
            direction.total_jobs += 1;
            direction.total_bytes = direction.total_bytes.saturating_add(job.total);
            direction.completed_bytes = direction.completed_bytes.saturating_add(done);
            if job.status == "completed" {
                direction.completed_jobs += 1;
            }
            match job.status.as_str() {
                "completed" => value.completed_jobs += 1,
                "running" => {
                    value.active_jobs += 1;
                    if entry.hashing {
                        value.hashing_jobs += 1;
                    }
                    if let Some(sample) = sample {
                        if job.upload {
                            value.upload_speed += sample.speed;
                        } else {
                            value.download_speed += sample.speed;
                        }
                    }
                }
                "queued" => value.queued_jobs += 1,
                "paused" => value.paused_jobs += 1,
                "failed" => value.failed_jobs += 1,
                _ => {}
            }
            if matches!(job.status.as_str(), "running" | "queued") {
                let remaining = job.total.saturating_sub(done);
                if job.upload {
                    value.upload_remaining = value.upload_remaining.saturating_add(remaining);
                } else {
                    value.download_remaining = value.download_remaining.saturating_add(remaining);
                }
            }
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn job(id: &str, status: &str) -> ActivityJob {
        ActivityJob {
            id: id.into(),
            upload: true,
            total: 100,
            completed: 0,
            status: status.into(),
        }
    }
    #[test]
    fn hashing_does_not_count_as_uploaded_bytes_or_network_speed() {
        let mut activity = TransferActivity::default();
        activity.observe(vec![job("a", "running")]);
        activity.progress("a", 80, 100, 1_000_000.0, "hashing");
        let overview = activity.overview();
        assert_eq!(overview.completed_bytes, 0);
        assert_eq!(overview.upload_speed, 0.0);
        assert_eq!(overview.hashing_jobs, 1);
    }
    #[test]
    fn completed_members_stay_in_batch_and_new_batches_reset_history() {
        let mut activity = TransferActivity::default();
        activity.observe(vec![job("a", "running"), job("b", "queued")]);
        activity.set_status("a", "completed");
        activity.observe(vec![job("b", "running")]);
        assert_eq!(activity.overview().byte_percent(), 50.0);
        activity.set_status("b", "completed");
        assert_eq!(activity.overview().job_percent(), 100.0);
        activity.observe(vec![job("c", "queued")]);
        assert_eq!(activity.overview().total_jobs, 1);
        assert_eq!(activity.overview().completed_bytes, 0);
    }
    #[test]
    fn paused_cancelled_and_stale_jobs_have_no_live_network_speed() {
        let mut activity = TransferActivity::default();
        activity.observe(vec![job("a", "running"), job("b", "running")]);
        activity.progress("a", 40, 100, 10.0, "transferring");
        activity.progress("b", 20, 100, 20.0, "transferring");
        activity.set_status("a", "paused");
        activity
            .jobs
            .get_mut("b")
            .unwrap()
            .sample
            .as_mut()
            .unwrap()
            .at = Instant::now() - Duration::from_secs(5);
        assert_eq!(activity.overview().upload_speed, 0.0);
        activity.set_status("b", "cancelled");
        assert_eq!(activity.overview().total_jobs, 1);
    }
}
