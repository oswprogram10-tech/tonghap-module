//! 1년치 데이터 성능 점검. 설계: `docs/design/16-year-scale-performance.md` 2절.
//!
//! 학급 25명이 1년 쓴 양(관찰 3,000건, 첨부 1,500개, 차시 400개, 습관 체크 18,000건)을 임시 폴더 DB에 만든 뒤
//! 각 명령의 핵심 함수를 5번씩 재서 중간값이 상한을 넘으면 실패한다. 상한은 "보통 PC에서 체감 지연 없음"이 기준이다.
//! 무거우므로 `#[ignore]`다. 실행(릴리스 빌드로 재야 의미가 있다):
//!   cargo test --manifest-path src-tauri/Cargo.toml --release --test perf -- --ignored --nocapture

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use tonghap_module_lib::testing::*;

const RUNS: usize = 5;

struct Row {
    name: &'static str,
    median: Duration,
    limit: Option<Duration>,
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn measure<T>(name: &'static str, limit_ms: Option<u64>, rows: &mut Vec<Row>, mut f: impl FnMut() -> T) {
    let _ = f(); // 첫 실행은 문장 준비·캐시 데우기라 센 값에서 뺀다. 앱도 같은 연결을 계속 쓴다.
    let times: Vec<Duration> = (0..RUNS)
        .map(|_| {
            let t = Instant::now();
            let _ = std::hint::black_box(f());
            t.elapsed()
        })
        .collect();
    rows.push(Row { name, median: median(times), limit: limit_ms.map(Duration::from_millis) });
}

fn work_dir() -> PathBuf {
    let p = std::env::temp_dir().join(format!("tonghap-perf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(p.join("media")).unwrap();
    p
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[test]
#[ignore = "1년치 데이터를 만들어 시간을 잰다. --ignored로 실행"]
fn year_scale_timings_stay_under_limits() {
    let dir = work_dir();
    let media = dir.join("media");
    let mut conn = Connection::open(dir.join("tonghap.db")).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    prepare(&conn).unwrap();
    let t = Instant::now();
    let s = seed_year(&mut conn, &media, &SeedOpts::default()).unwrap();
    println!("seed_year: {:.0}ms", ms(t.elapsed()));

    let student = s.student_ids[0].clone();
    let (from, to) = (s.first_day.clone(), s.last_day.clone());
    let mut rows: Vec<Row> = Vec::new();

    let base = observations::ObservationQuery { class_id: s.class_id.clone(), ..Default::default() };
    let recent = || observations::ObservationQuery { limit: Some(20), class_id: base.class_id.clone(), ..Default::default() };
    measure("observation_list (최근 20)", Some(50), &mut rows, || observations::list(&conn, &media, &recent()).unwrap());
    let search = || observations::ObservationQuery {
        class_id: s.class_id.clone(),
        text: Some("친구 도와".to_string()),
        from: Some("2025-06-01".to_string()),
        to: Some("2025-12-31".to_string()),
        limit: Some(20),
        ..Default::default()
    };
    measure("observation_list (검색어 2낱말+기간)", Some(150), &mut rows, || observations::list(&conn, &media, &search()).unwrap());
    measure("observation_count (같은 조건)", Some(150), &mut rows, || observations::count(&conn, &search()).unwrap());
    measure("portfolio_student", Some(100), &mut rows, || portfolio::student(&conn, &student).unwrap());
    measure("portfolio_class", Some(200), &mut rows, || portfolio::class(&conn, &s.class_id).unwrap());
    measure("unit_get (차시 40개)", Some(50), &mut rows, || units::get(&conn, &s.big_unit_id).unwrap());
    measure("habit_week", Some(50), &mut rows, || habits::week(&conn, &s.habit_ids[0], &s.habit_last_day).unwrap());
    measure("habit_student_summary (180일)", Some(50), &mut rows, || habits::student_summary(&conn, &student, &s.habit_last_day, 180).unwrap());
    measure("report_draft (1년)", Some(150), &mut rows, || reports::draft(&conn, &media, &student, None, &from, &to).unwrap());
    measure("evidence_bundle (바, 1년)", Some(150), &mut rows, || evidence_bundle(&conn, &media, &student, "바", &from, &to, &[]).unwrap());
    let rec = activities::RecommendInput { unit_id: s.big_unit_id.clone(), after_session_id: None, questions: None };
    measure("activity_recommend", Some(50), &mut rows, || activities::recommend(&conn, &rec).unwrap());
    measure("시작: 마이그레이션 확인+자막 대기 정리", Some(100), &mut rows, || {
        conn.execute("UPDATE attachments SET transcript_status = 'queued' WHERE id IN (SELECT id FROM attachments WHERE kind = 'audio' LIMIT 3)", []).unwrap();
        migrations::apply(&conn).unwrap();
        reset_pending(&conn).unwrap()
    });

    // 백업: 상한 없이 시간만 기록하고, 진행률 이벤트 간격을 본다.
    let shared = Mutex::new(conn);
    let out = dir.join("backup.tonghap");
    let mut events: Vec<(BackupProgress, Instant)> = Vec::new();
    let t = Instant::now();
    let r = backup_format::create(&shared, &dir, &out, "perf-password", Some(2), &mut |p| events.push((p, Instant::now()))).unwrap();
    let backup_time = t.elapsed();
    assert_eq!(r.counts.attachments, 1500);
    // 앱(`backup_create`)과 같은 솎기를 걸었을 때 화면으로 가는 이벤트 수와 가장 짧은 간격.
    let (sent, min_gap) = {
        let mut th = Throttle::new(PROGRESS_MIN_INTERVAL);
        let (mut n, mut last, mut min_gap) = (0usize, None::<Instant>, Duration::MAX);
        for (p, when) in &events {
            if th.pass_at(p, *when) {
                n += 1;
                if let Some(l) = last {
                    min_gap = min_gap.min(when.duration_since(l));
                }
                last = Some(*when);
            }
        }
        (n, min_gap)
    };

    println!("\n┌──────────────────────────────────────────┬──────────┬──────────┐");
    println!("│ {:<40} │ {:>8} │ {:>8} │", "대상 (중간값, 5회)", "측정(ms)", "상한(ms)");
    println!("├──────────────────────────────────────────┼──────────┼──────────┤");
    for r in &rows {
        let lim = r.limit.map_or("-".to_string(), |l| format!("{:.0}", ms(l)));
        let mark = if r.limit.is_some_and(|l| r.median > l) { " 초과" } else { "" };
        println!("│ {:<40} │ {:>8.2} │ {:>8} │{mark}", r.name, ms(r.median), lim);
    }
    println!("└──────────────────────────────────────────┴──────────┴──────────┘");
    println!("백업 만들기(첨부 1,500개): {:.0}ms, 진행률 이벤트 {}개 -> 화면 전달 {}개(단계 전환·끝 이벤트는 간격과 무관하게 전달, 최소 간격 {:.0}ms)", ms(backup_time), events.len(), sent, ms(min_gap));

    let over: Vec<String> = rows
        .iter()
        .filter(|r| r.limit.is_some_and(|l| r.median > l))
        .map(|r| format!("{} {:.1}ms > {:.0}ms", r.name, ms(r.median), ms(r.limit.unwrap())))
        .collect();
    drop(shared);
    // `TONGHAP_PERF_KEEP=1`이면 만든 DB를 남겨 `EXPLAIN QUERY PLAN`으로 들여다볼 수 있다.
    if std::env::var_os("TONGHAP_PERF_KEEP").is_some() {
        println!("남긴 폴더: {}", dir.display());
    } else {
        let _ = std::fs::remove_dir_all(&dir);
    }
    assert!(over.is_empty(), "상한을 넘은 곳: {over:?}");
    // 단계가 바뀌거나 끝나는 이벤트는 간격과 상관없이 나가므로, 그 외에는 100ms 이상이어야 한다.
    assert!(sent * 20 < events.len().max(20), "진행률 이벤트가 충분히 솎이지 않았습니다: {sent}/{}", events.len());
}
