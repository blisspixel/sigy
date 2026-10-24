SELECT EXISTS (
    SELECT 1
    FROM claims AS c
    WHERE NOT EXISTS (
            SELECT 1 FROM claim_revisions AS r
            WHERE r.claim_id = c.id AND r.revision = 1
        )
        OR (SELECT count(*) FROM claim_revisions AS r WHERE r.claim_id = c.id)
            != (SELECT max(revision) FROM claim_revisions AS r WHERE r.claim_id = c.id)
        OR EXISTS (
            SELECT 1 FROM claim_sightings AS s
            WHERE s.claim_id = c.id
              AND (
                  (
                      s.finding_id IS NOT NULL
                      AND NOT EXISTS (
                          SELECT 1 FROM monitor_findings AS f
                          WHERE f.monitor_id = s.monitor_id AND f.id = s.finding_id
                      )
                  )
                  OR (
                      s.transcript_id IS NOT NULL
                      AND NOT EXISTS (
                          SELECT 1
                          FROM transcript_cues AS cue
                          JOIN translation_cues AS tc
                            ON tc.transcript_id = cue.transcript_id
                           AND tc.transcript_revision = cue.revision
                           AND tc.revision = s.translation_revision
                           AND tc.ordinal = cue.ordinal
                          WHERE cue.transcript_id = s.transcript_id
                            AND cue.revision = s.transcript_revision
                            AND cue.ordinal = s.cue_ordinal
                            AND cue.start_us = s.start_us
                            AND cue.end_us = s.end_us
                      )
                  )
              )
        )
        OR EXISTS (
            SELECT 1 FROM claim_evidence AS e
            WHERE e.claim_id = c.id
              AND (
                  e.origin != 'user'
                  OR NOT EXISTS (
                      SELECT 1 FROM claim_sightings AS s
                      WHERE s.claim_id = e.claim_id AND s.id = e.sighting_id
                  )
              )
        )
        OR EXISTS (
            SELECT 1 FROM claim_status AS st
            WHERE st.claim_id = c.id
              AND (
                  st.setter != 'user'
                  OR NOT EXISTS (
                      SELECT 1 FROM claim_evidence AS e
                      WHERE e.claim_id = st.claim_id AND e.id = st.evidence_id
                  )
              )
        )
        OR (
            (SELECT count(*) FROM claim_status AS st WHERE st.claim_id = c.id) > 0
            AND (SELECT count(*) FROM claim_status AS st WHERE st.claim_id = c.id)
                != (SELECT max(revision) FROM claim_status AS st WHERE st.claim_id = c.id)
        )
);
