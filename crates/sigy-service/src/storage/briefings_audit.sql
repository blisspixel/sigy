SELECT EXISTS (
    SELECT 1
    FROM monitor_briefings AS b
    WHERE b.classification <> 'off'
       OR (SELECT count(*) FROM monitor_briefings AS counted WHERE counted.monitor_id = b.monitor_id) > 1024
       OR (SELECT max(generation) FROM monitor_briefings AS generations WHERE generations.monitor_id = b.monitor_id)
          <> (SELECT count(*) FROM monitor_briefings AS generations WHERE generations.monitor_id = b.monitor_id)
       OR b.corroboration <> (
            SELECT count(DISTINCT members.group_ordinal)
            FROM monitor_briefing_members AS members
            WHERE members.monitor_id = b.monitor_id AND members.briefing_id = b.id
       )
       OR (
            b.corroboration > 0 AND (
                (SELECT min(members.group_ordinal)
                 FROM monitor_briefing_members AS members
                 WHERE members.monitor_id = b.monitor_id AND members.briefing_id = b.id) <> 0
                OR (SELECT max(members.group_ordinal)
                    FROM monitor_briefing_members AS members
                    WHERE members.monitor_id = b.monitor_id AND members.briefing_id = b.id)
                   <> b.corroboration - 1
            )
       )
       OR NOT EXISTS (
            SELECT 1 FROM monitor_briefing_coverage AS coverage
            WHERE coverage.monitor_id = b.monitor_id AND coverage.briefing_id = b.id
       )
       OR EXISTS (
            SELECT 1 FROM monitor_briefing_sources AS source
            WHERE source.monitor_id = b.monitor_id AND source.briefing_id = b.id
              AND source.ordinal >= (
                    SELECT count(*) FROM monitor_briefing_sources AS counted
                    WHERE counted.monitor_id = source.monitor_id
                      AND counted.briefing_id = source.briefing_id
              )
       )
       OR EXISTS (
            SELECT 1 FROM monitor_briefing_schedules AS schedule
            WHERE schedule.monitor_id = b.monitor_id AND schedule.briefing_id = b.id
              AND schedule.ordinal >= (
                    SELECT count(*) FROM monitor_briefing_schedules AS counted
                    WHERE counted.monitor_id = schedule.monitor_id
                      AND counted.briefing_id = schedule.briefing_id
              )
       )
       OR EXISTS (
            SELECT 1 FROM monitor_briefing_reasons AS reason
            WHERE reason.monitor_id = b.monitor_id AND reason.briefing_id = b.id
              AND reason.ordinal >= (
                    SELECT count(*) FROM monitor_briefing_reasons AS counted
                    WHERE counted.monitor_id = reason.monitor_id
                      AND counted.briefing_id = reason.briefing_id
                      AND counted.source_ordinal = reason.source_ordinal
              )
       )
);
