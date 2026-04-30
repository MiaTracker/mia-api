use std::collections::{HashMap, HashSet};
use sea_orm::{ColumnTrait, ConnectionTrait, Database, DbConn, EntityTrait, FromQueryResult, QueryFilter, QuerySelect, TransactionTrait};
use sea_orm::ActiveValue::Set;
use sea_orm::prelude::Expr;
use sea_orm::ActiveModelTrait;
use uuid::Uuid;
use entities::{logs, media, users};
use entities::prelude::{Logs, Media, Users};
use infrastructure::config;
use views::ratings_linearization::{LinearizeResult, UserSummary};
use crate::infrastructure::SrvErr;

pub async fn run_linearize_ratings(user_uuid: Option<Uuid>, dry_run: bool) {
    crate::infrastructure::initialize().await;

    let db_url = config().db.connection_url.clone();
    let conn = Database::connect(db_url.clone()).await
        .unwrap_or_else(|_| panic!("Failed to connect to database using connection string \"{}\"", db_url));

    match linearize_ratings(&conn, user_uuid, dry_run).await {
        Ok(result) => {
            println!(
                "Linearization {} | processed: {}, skipped: {}, logs updated: {}, media recomputed: {}",
                if result.dry_run { "DRY RUN" } else { "APPLIED" },
                result.users_processed,
                result.users_skipped,
                result.logs_updated,
                result.media_recomputed,
            );
            println!("{:<36}  {:>5}  {:>5}  {:>8}  {:>8}  {:>9}  skip",
                "uuid", "n", "k", "avg_bef", "avg_aft", "max_delta");
            for u in &result.per_user {
                println!("{:<36}  {:>5}  {:>5}  {:>8}  {:>8}  {:>9}  {}",
                    u.user_uuid,
                    u.n,
                    u.k,
                    u.avg_before.map(|v| format!("{:.2}", v)).unwrap_or_default(),
                    u.avg_after.map(|v| format!("{:.2}", v)).unwrap_or_default(),
                    u.max_delta.map(|v| format!("{:.2}", v)).unwrap_or_default(),
                    u.skip_reason.as_deref().unwrap_or(""),
                );
            }
        }
        Err(err) => {
            eprintln!("Linearization failed: {}", err);
            std::process::exit(1);
        }
    }
}

pub async fn linearize_ratings(
    db: &DbConn,
    user_uuid: Option<Uuid>,
    dry_run: bool,
) -> Result<LinearizeResult, SrvErr> {
    let user_id = if let Some(uuid) = user_uuid {
        let user = Users::find()
            .filter(users::Column::Uuid.eq(uuid))
            .one(db)
            .await?;
        match user {
            Some(u) => Some(u.id),
            None => return Err(SrvErr::NotFound),
        }
    } else {
        None
    };

    let tran = db.begin().await?;

    let result = do_linearize(&tran, user_id, dry_run).await;

    match result {
        Ok(r) => {
            if dry_run {
                tran.rollback().await?;
            } else {
                tran.commit().await?;
            }
            Ok(r)
        }
        Err(e) => {
            let _ = tran.rollback().await;
            Err(e)
        }
    }
}

async fn do_linearize(
    conn: &impl ConnectionTrait,
    user_id: Option<i32>,
    dry_run: bool,
) -> Result<LinearizeResult, SrvErr> {
    #[derive(FromQueryResult)]
    struct UserRow {
        pub id: i32,
        pub uuid: Uuid,
    }

    let users_to_process: Vec<UserRow> = if let Some(uid) = user_id {
        Users::find()
            .filter(users::Column::Id.eq(uid))
            .select_only()
            .columns([users::Column::Id, users::Column::Uuid])
            .into_model::<UserRow>()
            .all(conn)
            .await?
    } else {
        Users::find()
            .select_only()
            .columns([users::Column::Id, users::Column::Uuid])
            .into_model::<UserRow>()
            .all(conn)
            .await?
    };

    #[derive(FromQueryResult)]
    struct LogRow {
        pub id: i32,
        pub media_id: i32,
        pub stars: f32,
    }

    let mut per_user: Vec<UserSummary> = Vec::new();
    let mut affected_media_ids: HashSet<i32> = HashSet::new();
    let mut total_logs_updated: u32 = 0;
    let mut users_processed: u32 = 0;
    let mut users_skipped: u32 = 0;

    for user in &users_to_process {
        // Fetch all logs with non-null stars for this user's media
        // Uses COALESCE(original_stars, stars) as the input value
        let log_rows: Vec<LogRow> = Logs::find()
            .inner_join(Media)
            .filter(media::Column::UserId.eq(user.id))
            .filter(logs::Column::Stars.is_not_null())
            .select_only()
            .column(logs::Column::Id)
            .column(logs::Column::MediaId)
            .column_as(
                Expr::cust("COALESCE(logs.original_stars, logs.stars)"),
                "stars",
            )
            .into_model::<LogRow>()
            .all(conn)
            .await?;

        if log_rows.is_empty() {
            continue;
        }

        let n = log_rows.len() as u32;

        // Build sorted distinct values and their multiplicities
        let mut value_counts: HashMap<u64, u32> = HashMap::new();
        for row in &log_rows {
            // Use bit representation to group equal floats
            let key = (row.stars as f64).to_bits();
            *value_counts.entry(key).or_insert(0) += 1;
        }

        let mut sorted_values: Vec<(f64, u32)> = value_counts
            .into_iter()
            .map(|(bits, count)| (f64::from_bits(bits), count))
            .collect();
        sorted_values.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        let k = sorted_values.len() as u32;

        // Skip if N < 3 or K < 3
        if n < 3 {
            per_user.push(UserSummary {
                user_id: user.id,
                user_uuid: user.uuid,
                n,
                k,
                skip_reason: Some("n_lt_3".to_string()),
                avg_before: None,
                avg_after: None,
                max_delta: None,
            });
            users_skipped += 1;
            continue;
        }
        if k < 3 {
            per_user.push(UserSummary {
                user_id: user.id,
                user_uuid: user.uuid,
                n,
                k,
                skip_reason: Some("k_lt_3".to_string()),
                avg_before: None,
                avg_after: None,
                max_delta: None,
            });
            users_skipped += 1;
            continue;
        }

        let multiplicities: Vec<u32> = sorted_values.iter().map(|(_, m)| *m).collect();
        let avg_before = log_rows.iter().map(|r| r.stars as f64).sum::<f64>() / n as f64;

        let positions = match solve_v4(&multiplicities) {
            Some(p) => p,
            None => {
                per_user.push(UserSummary {
                    user_id: user.id,
                    user_uuid: user.uuid,
                    n,
                    k,
                    skip_reason: Some("infeasible_unrecoverable".to_string()),
                    avg_before: Some(avg_before as f32),
                    avg_after: None,
                    max_delta: None,
                });
                users_skipped += 1;
                continue;
            }
        };

        // Map each original value to its new position
        let value_to_position: HashMap<u64, f64> = sorted_values
            .iter()
            .enumerate()
            .map(|(i, (val, _))| (val.to_bits(), positions[i]))
            .collect();

        let avg_after = positions
            .iter()
            .zip(sorted_values.iter())
            .map(|(pos, (_, mult))| pos * (*mult as f64))
            .sum::<f64>() / n as f64;

        let mut max_delta: f64 = 0.0;

        for row in &log_rows {
            let new_stars = value_to_position[&(row.stars as f64).to_bits()];
            let delta = (new_stars - row.stars as f64).abs();
            if delta > max_delta {
                max_delta = delta;
            }

            // Update: SET stars = new, original_stars = COALESCE(original_stars, stars)
            let log_model = Logs::find_by_id(row.id).one(conn).await?;
            if let Some(log_model) = log_model {
                let preserved_original = log_model.original_stars.or(log_model.stars);
                let mut am: logs::ActiveModel = log_model.into();
                am.stars = Set(Some(round2(new_stars) as f32));
                am.original_stars = Set(preserved_original);
                am.update(conn).await?;
            }

            affected_media_ids.insert(row.media_id);
            total_logs_updated += 1;
        }

        per_user.push(UserSummary {
            user_id: user.id,
            user_uuid: user.uuid,
            n,
            k,
            skip_reason: None,
            avg_before: Some(avg_before as f32),
            avg_after: Some(avg_after as f32),
            max_delta: Some(max_delta as f32),
        });
        users_processed += 1;
    }

    // Recompute media.stars for all affected media
    let media_recomputed = affected_media_ids.len() as u32;
    for media_id in &affected_media_ids {
        recompute_media_rating(*media_id, conn).await?;
    }

    Ok(LinearizeResult {
        dry_run,
        users_processed,
        users_skipped,
        logs_updated: total_logs_updated,
        media_recomputed,
        per_user,
    })
}

async fn recompute_media_rating(media_id: i32, conn: &impl ConnectionTrait) -> Result<(), SrvErr> {
    #[derive(FromQueryResult)]
    struct AvgSelect {
        pub sum: Option<f32>,
        pub count: i64,
    }

    let sel = Logs::find()
        .filter(logs::Column::MediaId.eq(media_id))
        .filter(logs::Column::Stars.is_not_null())
        .select_only()
        .column_as(Expr::col(logs::Column::Id).count(), "count")
        .column_as(Expr::col(logs::Column::Stars).sum(), "sum")
        .group_by(logs::Column::MediaId)
        .into_model::<AvgSelect>()
        .one(conn)
        .await?;

    let avg = sel.and_then(|s| s.sum.map(|sum| sum / s.count as f32));

    let media_model = Media::find_by_id(media_id).one(conn).await?;
    if let Some(media_model) = media_model {
        let mut am: media::ActiveModel = media_model.into();
        am.stars = Set(avg);
        am.update(conn).await?;
    }

    Ok(())
}

/// Solves the V4 QP for the given multiplicities (sorted ascending).
/// Returns positions p_1..p_K anchored at 0 and 10, rounded to 2 decimals.
/// Returns None if the problem is unrecoverable.
fn solve_v4(multiplicities: &[u32]) -> Option<Vec<f64>> {
    let k = multiplicities.len();
    assert!(k >= 3);

    let n: u32 = multiplicities.iter().sum();
    let nf = n as f64;

    // c_j = m_{j+1} + ... + m_K for j = 1..K-1
    let c: Vec<f64> = (0..k - 1)
        .map(|j| multiplicities[j + 1..].iter().map(|&m| m as f64).sum())
        .collect();

    let s1: f64 = c.iter().sum();
    let s2: f64 = c.iter().map(|&ci| ci * ci).sum();

    let target_avg = 5.0_f64;
    let positions = try_solve_qp(&c, s1, s2, nf, target_avg, k)?;

    Some(positions)
}

fn try_solve_qp(
    c: &[f64],
    s1: f64,
    s2: f64,
    nf: f64,
    target_avg: f64,
    k: usize,
) -> Option<Vec<f64>> {
    // Solve 2x2 system via Cramer's rule:
    // (K-1)*α - S1*β = 20
    //   S1*α - S2*β = 10*N*target_avg/5  (scaled: originally 10N for avg=5)
    let km1 = (k - 1) as f64;
    let rhs1 = 20.0_f64;
    let rhs2 = 2.0 * nf * target_avg;

    let det = km1 * (-s2) - (-s1) * s1;
    if det.abs() < 1e-12 {
        return None;
    }

    let alpha = (rhs1 * (-s2) - (-s1) * rhs2) / det;
    let beta = (km1 * rhs2 - rhs1 * s1) / det;

    let gaps: Vec<f64> = c.iter().map(|&ci| (alpha - beta * ci) / 2.0).collect();

    // Check feasibility
    let all_positive = gaps.iter().all(|&g| g > 0.0);
    if all_positive {
        return Some(build_positions(&gaps));
    }

    // Infeasibility fallback: find feasible avg interval and project
    // The feasible interval for avg is (10*m_K/N, 10*(N-m_1)/N)
    // We need to re-derive from the multiplicities. Since c[j] = sum of m_{j+1}..m_K,
    // c[0] = sum of m_2..m_K = N - m_1, c[K-2] = m_K
    let m1 = nf - c[0];
    let mk = c[c.len() - 1];

    let lo = 10.0 * mk / nf;
    let hi = 10.0 * (nf - m1) / nf;

    if lo >= hi - 1e-9 {
        return None;
    }

    const EPS: f64 = 1e-6;
    let relaxed_avg = 5.0_f64.max(lo + EPS).min(hi - EPS);

    let rhs2_relaxed = 2.0 * nf * relaxed_avg;
    let det2 = km1 * (-s2) - (-s1) * s1;
    if det2.abs() < 1e-12 {
        return None;
    }

    let alpha2 = (rhs1 * (-s2) - (-s1) * rhs2_relaxed) / det2;
    let beta2 = (km1 * rhs2_relaxed - rhs1 * s1) / det2;

    let gaps2: Vec<f64> = c.iter().map(|&ci| (alpha2 - beta2 * ci) / 2.0).collect();

    if gaps2.iter().all(|&g| g > 0.0) {
        Some(build_positions(&gaps2))
    } else {
        None
    }
}

fn build_positions(gaps: &[f64]) -> Vec<f64> {
    let mut positions = Vec::with_capacity(gaps.len() + 1);
    positions.push(0.0_f64);
    let mut acc = 0.0_f64;
    for &g in gaps {
        acc += g;
        positions.push(acc);
    }
    positions
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uniform_six_ratings() {
        // K=6, all multiplicity 1: expect uniform spacing 0, 2, 4, 6, 8, 10
        let mults = vec![1u32, 1, 1, 1, 1, 1];
        let positions = solve_v4(&mults).expect("should solve");
        assert_eq!(positions.len(), 6);
        assert_eq!(round2(positions[0]), 0.0);
        assert_eq!(round2(positions[5]), 10.0);
        // avg should be 5
        let avg: f64 = positions.iter().sum::<f64>() / 6.0;
        assert!((avg - 5.0).abs() < 0.1, "avg={}", avg);
        // gaps should be ~2 each
        for i in 1..6 {
            let gap = positions[i] - positions[i - 1];
            assert!((gap - 2.0).abs() < 0.1, "gap[{}]={}", i, gap);
        }
    }

    #[test]
    fn test_uneven_multiplicities() {
        // K=8, multiplicities [1,1,1,1,1,2,1,1] — from V4 design discussion
        let mults = vec![1u32, 1, 1, 1, 1, 2, 1, 1];
        let positions = solve_v4(&mults).expect("should solve");
        assert_eq!(positions.len(), 8);
        assert_eq!(round2(positions[0]), 0.0);
        assert_eq!(round2(positions[7]), 10.0);
        let n: u32 = mults.iter().sum();
        let avg: f64 = positions.iter().zip(mults.iter())
            .map(|(p, &m)| p * m as f64)
            .sum::<f64>() / n as f64;
        assert!((avg - 5.0).abs() < 0.1, "avg={}", avg);
    }

    #[test]
    fn test_infeasible_fallback() {
        // K=3, m_1 > N/2: [6, 2, 2] — should trigger fallback
        let mults = vec![6u32, 2, 2];
        let positions = solve_v4(&mults).expect("should solve with fallback");
        assert_eq!(positions.len(), 3);
        assert_eq!(round2(positions[0]), 0.0);
        assert_eq!(round2(positions[2]), 10.0);
        // All gaps must be positive
        for i in 1..3 {
            assert!(positions[i] > positions[i - 1], "gap must be positive at {}", i);
        }
    }
}
