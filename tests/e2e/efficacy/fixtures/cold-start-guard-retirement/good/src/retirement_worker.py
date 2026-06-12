"""Retirement worker with cold-start guard."""


def get_retirement_candidates(conn, threshold: int) -> list:
    """
    Return skills whose usage count is below the threshold, excluding
    never-used skills (cold-start guard).

    Items with zero historical usage are excluded — they may be newly seeded
    and indistinguishable from genuinely abandoned items on first boot.
    """
    rows = conn.execute(
        "SELECT skill_id, total_usage_count FROM skills "
        "WHERE total_usage_count > 0 AND total_usage_count < %s",
        (threshold,)
    ).fetchall()
    return [r[0] for r in rows]
