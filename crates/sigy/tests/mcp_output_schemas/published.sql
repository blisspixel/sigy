-- Synthetic completed provenance with two original cues: one translated, one
-- explicitly untranslated. No media or native executable is needed to read it.
INSERT INTO capture_jobs VALUES ('one','radio:v1',1000,61000,600,'completed',3,1,1,4);
INSERT INTO capture_events VALUES
 ('one',0,0,NULL,'scheduled',NULL,'accepted',1),
 ('one',1,1,'scheduled','starting','start','fixture',2),
 ('one',2,1,'starting','stopping','stop','fixture',3),
 ('one',3,1,'stopping','completed','finalized','fixture',4);
INSERT INTO recordings(id,object_key,duration_seconds,initial_retention,retention,storage_state,charged_bytes,media_bytes,sha256,format,decoded_microseconds,end_reason,metadata_requested,profile,byte_ceiling)
 VALUES ('one','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',60,'kept','kept','retained',100,100,
 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','wav',1000000,'end_of_body',0,'radio',600);
INSERT INTO recording_intervals VALUES ('one',0,0,1000000,0,100,'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','wav',600);
INSERT INTO recording_segment_clocks VALUES ('one',0,4);
INSERT INTO analysis_inputs VALUES ('pin',1,'one','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
 '{"planned_us":60000000,"intervals":[{"ordinal":0,"start_us":0,"end_us":1000000,"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}],"gaps":[]}', 'published',10);
INSERT INTO analysis_jobs(id,generation,analysis_id,analysis_revision,recording_id,profile,kind,profile_sha256,expected_parent_revision,state,expected_bytes,expected_files,manifest_sha256,amount_micros,created_ms,finished_ms,lineage)
 VALUES ('asr',1,'pin',1,'one','synthetic-storage-fixture-v1','local_asr','bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',0,'succeeded',100,1,'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',0,20,21,'pin');
INSERT INTO transcripts VALUES ('pin',1,'pin',1,'one','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
 'original','synthetic-storage-fixture-v1','recognition','text',NULL,'asr',1,'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',2,35,'published',21);
INSERT INTO transcript_coverage VALUES ('pin',1,0,0,0,1000000,'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',16000,16000);
INSERT INTO transcript_cues VALUES ('pin',1,0,0,500000,'Una feria mundial','uncertain'),('pin',1,1,500000,1000000,'Una feria regional','uncertain');
INSERT INTO analysis_decisions VALUES ('pin',1,0,NULL,21);
INSERT INTO translation_jobs(id,generation,transcript_id,transcript_revision,profile,profile_sha256,state,created_ms,finished_ms,lineage)
 VALUES ('mt-one',1,'pin',1,'mt','dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd','succeeded',30,31,'pin');
INSERT INTO translations VALUES ('pin',1,1,'mt-one',1,'mt','dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd','en',2,1,0,31);
INSERT INTO translation_cues VALUES ('pin',1,1,0,'translated','A world fair',NULL),('pin',1,1,1,'untranslated',NULL,'unsupported-language');
INSERT INTO job_interests VALUES ('recognition','asr','direct','','admitted',20),('translation','mt-one','direct','','admitted',30);
