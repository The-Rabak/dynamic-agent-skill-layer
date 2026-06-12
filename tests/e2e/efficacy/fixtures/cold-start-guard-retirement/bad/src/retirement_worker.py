"""Retirement worker without cold-start guard — mass-deletes on first boot."""


def get_retirement_candidates(conn, threshold: int) -> list:
    """
    Return skills whose usage count is below the threshold.

    BUG: includes never-used items (total_usage_count=0), causing
    mass-deletion of all newly-seeded skills on first deployment.
    """
    rows = conn.execute(
        "SELECT skill_id, total_usage_count FROM skills "
        "WHERE total_usage_count < %s",
        (threshold,)
    ).fetchall()
    return [r[0] for r in rows]
