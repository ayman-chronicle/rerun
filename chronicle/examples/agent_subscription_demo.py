"""
Chronicle Agent Subscription Demo
===================================

An AI agent subscribes to events for a specific customer and reacts
in real-time as events arrive from different SaaS sources. The agent:

1. Detects a payment failure
2. Automatically links it to the support ticket that follows
3. Flags the account as at-risk when escalation occurs
4. Prints a live investigation trace

No polling. No cron jobs. Pure push-based reactive intelligence.

Usage:
    python examples/agent_subscription_demo.py
"""

import json
import chronicle_py as chronicle

# ─────────────────────────────────────────────────────────
# The AI agent's state
# ─────────────────────────────────────────────────────────

class InvestigationAgent:
    """A simple reactive agent that watches a customer's event stream.

    The callback records observations and queues actions. The main loop
    processes the action queue after each log() call — this avoids
    calling Chronicle APIs from inside the subscription callback.
    """

    def __init__(self, customer_id):
        self.customer_id = customer_id
        self.events_seen = []
        self.pending_links = []
        self.last_payment_failure = None
        self.risk_level = "normal"

    def on_event(self, event):
        """Called for every event matching the subscription filter."""
        source = event["source"]
        etype = event["event_type"]
        self.events_seen.append(event)

        print(f"    [{source}] {etype}")

        if etype == "payment_intent.failed":
            self.last_payment_failure = event["event_id"]
            self.risk_level = "elevated"
            print(f"      >> Agent: Payment failure detected. Risk → {self.risk_level}")

        elif etype == "ticket.created" and self.last_payment_failure:
            self.pending_links.append((
                self.last_payment_failure,
                event["event_id"],
                "caused_by",
                0.9,
                "Payment failure preceded support ticket by minutes",
            ))
            print(f"      >> Agent: Queued auto-link payment_failed → ticket")

        elif etype == "ticket.escalated":
            self.risk_level = "high"
            print(f"      >> Agent: Escalation detected. Risk → {self.risk_level}")

        elif etype == "subscription.cancelled":
            self.risk_level = "churned"
            print(f"      >> Agent: Customer churned. Final risk → {self.risk_level}")

    def process_pending(self, ch):
        """Create any queued links. Call after each log() batch."""
        for src, tgt, ltype, conf, reason in self.pending_links:
            link_id = ch.create_link(src, tgt, link_type=ltype, confidence=conf, reasoning=reason)
            print(f"      >> Agent: Created link {link_id[:12]}… ({ltype})")
        created = len(self.pending_links)
        self.pending_links.clear()
        return created


def main():
    ch = chronicle.Chronicle.in_memory()
    agent = InvestigationAgent("cust_042")

    print("=" * 60)
    print("  Chronicle Agent Subscription Demo")
    print("=" * 60)

    # ─────────────────────────────────────────────────────
    # Step 1: Agent subscribes to customer events
    # ─────────────────────────────────────────────────────

    print("\n─── Step 1: Agent subscribes to customer cust_042 ───\n")

    handle = ch.subscribe(
        agent.on_event,
        entity_type="customer",
        entity_id="cust_042",
    )
    print("  Subscription active. Waiting for events…\n")

    # ─────────────────────────────────────────────────────
    # Step 2: Events arrive from different sources
    # ─────────────────────────────────────────────────────

    print("─── Step 2: Events arrive (agent reacts in real-time) ───\n")

    ch.log("stripe", "payments", "payment_intent.succeeded",
           entities={"customer": "cust_042"},
           payload={"amount": 4999, "currency": "usd"})

    ch.log("stripe", "payments", "payment_intent.failed",
           entities={"customer": "cust_042"},
           payload={"amount": 4999, "failure_code": "card_declined"})

    ch.log("support", "tickets", "ticket.created",
           entities={"customer": "cust_042", "ticket": "tkt_891"},
           payload={"subject": "Card declined", "priority": "high"})
    agent.process_pending(ch)

    ch.log("support", "tickets", "ticket.escalated",
           entities={"customer": "cust_042", "ticket": "tkt_891"},
           payload={"reason": "payment_issue", "escalated_to": "team_lead"})

    ch.log("billing", "subscriptions", "subscription.cancelled",
           entities={"customer": "cust_042", "account": "acc_7"},
           payload={"reason": "payment_failure", "mrr_lost": 4999})

    # ─────────────────────────────────────────────────────
    # Step 3: Summary
    # ─────────────────────────────────────────────────────

    print(f"\n─── Step 3: Agent summary ───\n")
    print(f"  Events observed: {len(agent.events_seen)}")
    print(f"  Final risk level: {agent.risk_level}")
    print(f"  Auto-links created: 1 (payment_failed → ticket.created)")

    # ─────────────────────────────────────────────────────
    # Step 4: Cancel subscription
    # ─────────────────────────────────────────────────────

    print(f"\n─── Step 4: Cancel subscription ───\n")
    handle.cancel()
    print("  Subscription cancelled. Agent stops receiving events.")

    # ─────────────────────────────────────────────────────
    # Step 5: Verify the timeline has the auto-linked events
    # ─────────────────────────────────────────────────────

    print(f"\n─── Step 5: Customer timeline (cross-source) ───\n")
    timeline = json.loads(ch.timeline("customer", "cust_042"))
    for i, evt in enumerate(timeline, 1):
        e = evt["event"]
        print(f"  {i}. [{e['source']}] {e['event_type']}")

    print("\n" + "=" * 60)
    print("  The agent reacted to 5 events across 3 sources,")
    print("  auto-linked the payment failure to the ticket,")
    print("  and tracked the customer's risk in real-time.")
    print("  No polling. No cron. Pure push.")
    print("=" * 60 + "\n")


if __name__ == "__main__":
    main()
