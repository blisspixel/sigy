-- One row per recognition group that was proven empty. Both measures stay null when
-- the mechanism does not account for an empty group. The rows are not a host budget.
CREATE TABLE worker_observations (
    job_id TEXT NOT NULL REFERENCES analysis_jobs(id),
    generation INTEGER NOT NULL CHECK(generation BETWEEN 1 AND 64),
    role TEXT NOT NULL CHECK(role IN ('decode', 'recognize')),
    ordinal INTEGER NOT NULL CHECK(ordinal >= 0 AND ordinal <= 1023),
    mechanism TEXT NOT NULL CHECK(mechanism IN ('job_object', 'cgroup_v2', 'process_group', 'process_reaper')),
    peak_memory_bytes INTEGER CHECK(peak_memory_bytes IS NULL OR peak_memory_bytes >= 0),
    cpu_time_us INTEGER CHECK(cpu_time_us IS NULL OR cpu_time_us >= 0),
    PRIMARY KEY(job_id, generation, role, ordinal)
) STRICT;
CREATE TRIGGER worker_observations_no_update BEFORE UPDATE ON worker_observations
BEGIN SELECT RAISE(ABORT, 'worker observations are immutable'); END;
CREATE TRIGGER worker_observations_no_delete BEFORE DELETE ON worker_observations
BEGIN SELECT RAISE(ABORT, 'worker observations are retained'); END;
PRAGMA user_version = 35;
