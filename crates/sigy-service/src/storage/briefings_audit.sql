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
);
