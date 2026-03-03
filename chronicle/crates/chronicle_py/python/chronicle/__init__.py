"""Chronicle: AI-native SaaS event store.

Usage:
    import chronicle

    ch = chronicle.Chronicle.in_memory()
    ch.log("stripe", "payments", "payment_intent.succeeded",
           entities={"customer": "cust_123"},
           payload={"amount": 4999})

    # JSON results
    results = ch.query(source="stripe")
    timeline = ch.timeline("customer", "cust_123")

    # DataFrame results (requires pyarrow)
    table = ch.read_df(ch.query_df(source="stripe"))
    df = table.to_pandas()

    ch.link_entity("session", "sess_1", "customer", "cust_123")
    tools = ch.agent_tools()
"""

from chronicle_py import Chronicle


def read_df(ipc_bytes):
    """Convert Arrow IPC bytes from query_df/timeline_df to a PyArrow Table.

    Usage:
        table = chronicle.read_df(ch.query_df(source="stripe"))
        df = table.to_pandas()
        df = table.to_pyarrow()  # or polars.from_arrow(table)
    """
    import pyarrow as pa

    reader = pa.ipc.open_stream(ipc_bytes)
    return reader.read_all()


# Attach helper to Chronicle class for convenience.
Chronicle.read_df = staticmethod(read_df)

__all__ = ["Chronicle", "read_df"]
