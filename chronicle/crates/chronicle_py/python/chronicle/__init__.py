"""Chronicle: AI-native SaaS event store.

Usage:
    import chronicle

    ch = chronicle.Chronicle.in_memory()
    ch.log("stripe", "payments", "payment_intent.succeeded",
           entities={"customer": "cust_123"},
           payload={"amount": 4999})

    results = ch.query(source="stripe")
    timeline = ch.timeline("customer", "cust_123")
    ch.link_entity("session", "sess_1", "customer", "cust_123")
    tools = ch.agent_tools()
"""

from chronicle_py import Chronicle

__all__ = ["Chronicle"]
