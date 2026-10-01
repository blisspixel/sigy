WITH lifetime AS (
    SELECT total_cap, byte_cap, policy_version, action_ordinal,
        LAG(policy_version) OVER (PARTITION BY monitor_id ORDER BY ordinal) AS previous_policy,
        LAG(action_ordinal) OVER (PARTITION BY monitor_id ORDER BY ordinal) AS previous_action,
        SUM(planned_seconds) OVER (PARTITION BY monitor_id ORDER BY ordinal ROWS UNBOUNDED PRECEDING) AS seconds,
        SUM(maximum_bytes) OVER (PARTITION BY monitor_id ORDER BY ordinal ROWS UNBOUNDED PRECEDING) AS bytes
    FROM monitor_capture_admissions
), daily AS (
    SELECT a.daily_cap,
        SUM(d.seconds) OVER (PARTITION BY a.monitor_id, d.utc_day ORDER BY a.ordinal ROWS UNBOUNDED PRECEDING) AS seconds
    FROM monitor_capture_days d JOIN monitor_capture_admissions a ON a.occurrence_id = d.occurrence_id
)
SELECT
    EXISTS(SELECT 1 FROM monitor_capture_admissions GROUP BY monitor_id HAVING MIN(ordinal) != 1 OR MAX(ordinal) != COUNT(*))
    OR
    EXISTS(SELECT 1 FROM monitor_capture_admissions a WHERE NOT EXISTS(
        SELECT 1 FROM schedule_occurrences o
        JOIN schedule_rules r ON r.id = o.rule_id
        JOIN monitor_capture_rules owner ON owner.rule_id = r.id
        JOIN monitor_versions v ON v.monitor_id = a.monitor_id AND v.version = a.policy_version
        WHERE o.id = a.occurrence_id AND o.state = 'admitted' AND o.recording_id = o.id
          AND owner.monitor_id = a.monitor_id AND r.source_revision = a.source_revision
          AND a.policy_version >= owner.created_version AND a.rule_revision <= r.revision
          AND o.rule_revision = a.rule_revision AND o.start_ms = a.start_ms AND o.end_ms = a.end_ms
          AND o.duration_seconds = a.planned_seconds AND o.maximum_bytes = a.maximum_bytes
          AND v.spec_sha256 = a.policy_sha256 AND a.end_ms - a.start_ms = a.planned_seconds * 1000
          AND json_extract(v.spec_json, '$.capture.daily_seconds') = a.daily_cap
          AND json_extract(v.spec_json, '$.capture.total_seconds') = a.total_cap
          AND json_extract(v.spec_json, '$.capture.total_bytes') = a.byte_cap
          AND (EXISTS(SELECT 1 FROM json_each(v.spec_json, '$.sources') WHERE value = a.source_revision) OR EXISTS(SELECT 1 FROM json_each(v.spec_json, '$.candidate_sources') WHERE value = a.source_revision))
          AND (a.action_ordinal = 0 OR EXISTS(SELECT 1 FROM monitor_actions WHERE monitor_id = a.monitor_id AND ordinal = a.action_ordinal AND policy_version <= a.policy_version))
          AND COALESCE((SELECT kind = 'add_source' FROM monitor_actions WHERE monitor_id = a.monitor_id AND policy_version = a.policy_version AND ordinal <= a.action_ordinal AND decision = 'applied' AND kind IN ('add_source', 'remove_source') AND json_extract(proposal_json, '$.source') = a.source_revision ORDER BY ordinal DESC LIMIT 1), EXISTS(SELECT 1 FROM json_each(v.spec_json, '$.sources') WHERE value = a.source_revision))
    ))
    OR EXISTS(SELECT 1 FROM schedule_occurrences o JOIN monitor_capture_rules owner ON owner.rule_id = o.rule_id WHERE o.state = 'admitted' AND NOT EXISTS(SELECT 1 FROM monitor_capture_admissions a WHERE a.occurrence_id = o.id))
    OR EXISTS(SELECT 1 FROM schedule_occurrences o JOIN schedule_rules r ON r.id = o.rule_id JOIN monitor_capture_rules owner ON owner.rule_id = r.id WHERE o.state = 'waiting' AND (o.rule_revision != r.revision OR o.duration_seconds != r.duration_seconds OR o.maximum_bytes != r.maximum_bytes))
    OR EXISTS(SELECT 1 FROM lifetime WHERE seconds > total_cap OR bytes > byte_cap OR policy_version < previous_policy OR action_ordinal < previous_action)
    OR EXISTS(SELECT 1 FROM daily WHERE seconds > daily_cap)
    OR EXISTS(SELECT 1 FROM monitor_capture_refusals denied WHERE NOT EXISTS(
        SELECT 1 FROM monitor_capture_rules owner JOIN schedule_rules r ON r.id = owner.rule_id
        WHERE owner.rule_id = denied.rule_id AND owner.monitor_id = denied.monitor_id
          AND denied.policy_version >= owner.created_version
          AND denied.source_revision = r.source_revision AND denied.rule_revision <= r.revision)
        OR denied.end_ms - denied.start_ms != denied.planned_seconds * 1000)
