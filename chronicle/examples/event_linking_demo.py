"""
Chronicle Event Linking Demo
=============================

Demonstrates Chronicle's causal event linking across multiple SaaS sources.

Scenario: A customer's payment fails, triggering a support ticket, which
gets escalated, leading to a subscription cancellation. Chronicle captures
this causal chain across Stripe, Support, and Billing systems — then lets
you trace the full story from any point.

Usage:
    python examples/event_linking_demo.py
"""

import json
import chronicle_py as chronicle

def main():
    ch = chronicle.Chronicle.in_memory()

    print("=" * 60)
    print("  Chronicle Event Linking Demo")
    print("=" * 60)

    # ─────────────────────────────────────────────────────────
    # 1. Log events from multiple SaaS sources
    # ─────────────────────────────────────────────────────────

    print("\n─── Step 1: Log events across 4 sources ───\n")

    # Stripe: payment lifecycle
    payment_ok = ch.log(
        "stripe", "payments", "payment_intent.succeeded",
        entities={"customer": "cust_042", "account": "acc_7"},
        payload={"amount": 4999, "currency": "usd", "plan": "pro"},
    )
    print(f"  ✓ stripe/payment_intent.succeeded  → {payment_ok[:12]}…")

    payment_fail = ch.log(
        "stripe", "payments", "payment_intent.failed",
        entities={"customer": "cust_042", "account": "acc_7"},
        payload={"amount": 4999, "currency": "usd", "failure_code": "card_declined"},
    )
    print(f"  ✓ stripe/payment_intent.failed     → {payment_fail[:12]}…")

    # Support: ticket lifecycle
    ticket = ch.log(
        "support", "tickets", "ticket.created",
        entities={"customer": "cust_042", "ticket": "tkt_891"},
        payload={"subject": "Payment failed — card declined", "priority": "high"},
    )
    print(f"  ✓ support/ticket.created           → {ticket[:12]}…")

    escalation = ch.log(
        "support", "tickets", "ticket.escalated",
        entities={"customer": "cust_042", "ticket": "tkt_891"},
        payload={"reason": "payment_issue", "escalated_to": "team_lead"},
    )
    print(f"  ✓ support/ticket.escalated         → {escalation[:12]}…")

    # Billing: subscription cancellation
    cancellation = ch.log(
        "billing", "subscriptions", "subscription.cancelled",
        entities={"customer": "cust_042", "account": "acc_7"},
        payload={"reason": "payment_failure", "plan": "pro", "mrr_lost": 4999},
    )
    print(f"  ✓ billing/subscription.cancelled   → {cancellation[:12]}…")

    # Product: the customer's last session before churning
    session = ch.log(
        "product", "usage", "session.ended",
        entities={"customer": "cust_042"},
        payload={"duration_s": 12, "pages_viewed": 1, "last_page": "/billing"},
    )
    print(f"  ✓ product/session.ended            → {session[:12]}…")

    print(f"\n  Logged 6 events across 4 sources for customer cust_042.\n")

    # ─────────────────────────────────────────────────────────
    # 2. Create causal links (the magic)
    # ─────────────────────────────────────────────────────────

    print("─── Step 2: Create causal links between events ───\n")

    link1 = ch.create_link(
        payment_fail, ticket,
        link_type="caused_by",
        confidence=0.92,
        reasoning="Payment failure led to support ticket within 2 hours",
    )
    print(f"  ✓ payment_failed  ──caused_by──▶  ticket.created     (0.92)")

    link2 = ch.create_link(
        ticket, escalation,
        link_type="led_to",
        confidence=0.95,
        reasoning="Unresolved payment ticket escalated to team lead",
    )
    print(f"  ✓ ticket.created  ──led_to────▶  ticket.escalated   (0.95)")

    link3 = ch.create_link(
        escalation, cancellation,
        link_type="caused_by",
        confidence=0.88,
        reasoning="Escalated ticket preceded subscription cancellation by 3 days",
    )
    print(f"  ✓ ticket.escalated ──caused_by──▶ sub.cancelled     (0.88)")

    link4 = ch.create_link(
        payment_fail, cancellation,
        link_type="led_to",
        confidence=0.75,
        reasoning="Payment failure was the root cause of churn",
    )
    print(f"  ✓ payment_failed  ──led_to────▶  sub.cancelled      (0.75)")

    print(f"\n  Created 4 causal links forming a churn chain.\n")

    # ─────────────────────────────────────────────────────────
    # 3. Query: customer timeline (cross-source!)
    # ─────────────────────────────────────────────────────────

    print("─── Step 3: Cross-source customer timeline ───\n")

    timeline = json.loads(ch.timeline("customer", "cust_042"))
    print(f"  Timeline for customer cust_042: {len(timeline)} events\n")
    for i, event in enumerate(timeline, 1):
        e = event["event"]
        print(f"  {i}. [{e['source']}] {e['event_type']}")

    # ─────────────────────────────────────────────────────────
    # 4. Query: filter by source
    # ─────────────────────────────────────────────────────────

    print("\n─── Step 4: Filter queries ───\n")

    stripe_events = json.loads(ch.query(source="stripe"))
    print(f"  Stripe events: {len(stripe_events)}")

    support_events = json.loads(ch.query(source="support"))
    print(f"  Support events: {len(support_events)}")

    billing_events = json.loads(ch.query(source="billing"))
    print(f"  Billing events: {len(billing_events)}")

    # ─────────────────────────────────────────────────────────
    # 5. JIT entity linking (the AI agent power)
    # ─────────────────────────────────────────────────────────

    print("\n─── Step 5: JIT entity linking ───\n")

    # Log an anonymous session from before the customer signed up
    anon_session = ch.log(
        "product", "sessions", "session.started",
        entities={"session": "sess_xyz"},
        payload={"referrer": "google_ads", "landing_page": "/pricing"},
    )
    print(f"  ✓ Logged anonymous session: sess_xyz")

    # An AI agent discovers this session belongs to cust_042
    linked = ch.link_entity("session", "sess_xyz", "customer", "cust_042")
    print(f"  ✓ AI agent linked session sess_xyz → customer cust_042 ({linked} events linked)")

    # Now the customer timeline includes the anonymous session!
    timeline_after = json.loads(ch.timeline("customer", "cust_042"))
    print(f"\n  Timeline after JIT linking: {len(timeline_after)} events (was {len(timeline)})")
    for i, event in enumerate(timeline_after, 1):
        e = event["event"]
        marker = " ← NEW (JIT linked)" if e["event_type"] == "session.started" else ""
        print(f"  {i}. [{e['source']}] {e['event_type']}{marker}")

    # ─────────────────────────────────────────────────────────
    # 6. Entity discovery
    # ─────────────────────────────────────────────────────────

    print("\n─── Step 6: Entity discovery ───\n")

    entity_types = json.loads(ch.describe_entity_types())
    for et in entity_types:
        print(f"  {et['entity_type']}: {et['entity_count']} entities")

    # ─────────────────────────────────────────────────────────
    # 7. AI agent tool definitions
    # ─────────────────────────────────────────────────────────

    print("\n─── Step 7: AI agent tools (OpenAI function calling) ───\n")

    tools = json.loads(ch.agent_tools())
    for tool in tools:
        name = tool["function"]["name"]
        desc = tool["function"]["description"]
        print(f"  • {name}: {desc}")

    print("\n" + "=" * 60)
    print("  The causal chain: payment failure → ticket → escalation → churn")
    print("  All discoverable from any point, across any source.")
    print("=" * 60 + "\n")


if __name__ == "__main__":
    main()
