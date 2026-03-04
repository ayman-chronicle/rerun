"""
Chronicle Stripe Connector Demo
================================

Demonstrates first-class Stripe support: one-liner webhook ingestion
with automatic entity ref extraction, topic derivation, and cross-source
timeline queries.

Usage:
    python examples/stripe_connector_demo.py
"""

import json
import sys
import chronicle_py as chronicle


# -- Realistic Stripe webhook fixtures --

PAYMENT_SUCCEEDED = json.dumps({
    "id": "evt_pay1",
    "type": "payment_intent.succeeded",
    "created": 1709481600,
    "data": {
        "object": {
            "id": "pi_abc123",
            "object": "payment_intent",
            "amount": 4999,
            "currency": "usd",
            "status": "succeeded",
            "customer": "cus_042",
            "payment_method": "pm_card_visa",
            "metadata": {"plan": "pro"}
        }
    }
})

CHARGE_FAILED = json.dumps({
    "id": "evt_fail1",
    "type": "charge.failed",
    "created": 1709568000,
    "data": {
        "object": {
            "id": "ch_fail1",
            "object": "charge",
            "amount": 4999,
            "currency": "usd",
            "status": "failed",
            "customer": "cus_042",
            "payment_intent": "pi_retry1",
            "failure_code": "card_declined",
            "failure_message": "Your card was declined"
        }
    }
})

SUBSCRIPTION_CREATED = json.dumps({
    "id": "evt_sub1",
    "type": "customer.subscription.created",
    "created": 1709654400,
    "data": {
        "object": {
            "id": "sub_pro_monthly",
            "object": "subscription",
            "status": "active",
            "customer": "cus_042",
            "current_period_start": 1709654400,
            "current_period_end": 1712332800,
            "cancel_at_period_end": False
        }
    }
})

INVOICE_PAID = json.dumps({
    "id": "evt_inv1",
    "type": "invoice.paid",
    "created": 1709740800,
    "data": {
        "object": {
            "id": "in_monthly_001",
            "object": "invoice",
            "amount_due": 4999,
            "amount_paid": 4999,
            "currency": "usd",
            "status": "paid",
            "customer": "cus_042",
            "subscription": "sub_pro_monthly",
            "billing_reason": "subscription_cycle"
        }
    }
})

DISPUTE_CREATED = json.dumps({
    "id": "evt_disp1",
    "type": "charge.dispute.created",
    "created": 1709827200,
    "data": {
        "object": {
            "id": "dp_fraud1",
            "object": "dispute",
            "amount": 4999,
            "currency": "usd",
            "status": "needs_response",
            "reason": "fraudulent",
            "charge": "ch_original"
        }
    }
})


def assert_eq(actual, expected, msg):
    if actual != expected:
        print(f"  FAIL: {msg}")
        print(f"    expected: {expected}")
        print(f"    actual:   {actual}")
        sys.exit(1)
    print(f"  PASS: {msg}")


def main():
    ch = chronicle.Chronicle.in_memory()
    passed = 0

    print("=" * 60)
    print("  Chronicle Stripe Connector Demo")
    print("=" * 60)

    # ── Step 1: Ingest Stripe webhooks with one-liners ──

    print("\n─── Step 1: Ingest Stripe webhooks ───\n")

    id1 = ch.ingest_stripe(PAYMENT_SUCCEEDED)
    print(f"  payment_intent.succeeded → {id1[:16]}…")

    id2 = ch.ingest_stripe(CHARGE_FAILED)
    print(f"  charge.failed            → {id2[:16]}…")

    id3 = ch.ingest_stripe(SUBSCRIPTION_CREATED)
    print(f"  subscription.created     → {id3[:16]}…")

    id4 = ch.ingest_stripe(INVOICE_PAID)
    print(f"  invoice.paid             → {id4[:16]}…")

    id5 = ch.ingest_stripe(DISPUTE_CREATED)
    print(f"  dispute.created          → {id5[:16]}…")

    print(f"\n  Ingested 5 Stripe webhooks.\n")

    # ── Step 2: Query by source ──

    print("─── Step 2: Query all Stripe events ───\n")

    stripe_events = json.loads(ch.query(source="stripe"))
    assert_eq(len(stripe_events), 5, "5 Stripe events ingested")
    passed += 1

    # ── Step 3: Verify topics are auto-derived ──

    print("\n─── Step 3: Verify topic auto-derivation ───\n")

    topics = {e["event"]["topic"] for e in stripe_events}
    assert_eq("payments" in topics, True, "payments topic present")
    assert_eq("subscriptions" in topics, True, "subscriptions topic present")
    assert_eq("invoices" in topics, True, "invoices topic present")
    assert_eq("disputes" in topics, True, "disputes topic present")
    passed += 4

    # ── Step 4: Cross-source customer timeline ──

    print("\n─── Step 4: Customer timeline (cross-source from Stripe) ───\n")

    timeline = json.loads(ch.timeline("customer", "cus_042"))
    print(f"  Timeline for customer cus_042: {len(timeline)} events\n")
    for i, evt in enumerate(timeline, 1):
        e = evt["event"]
        print(f"  {i}. [{e['source']}] {e['event_type']} ({e['topic']})")

    assert_eq(len(timeline), 4, "4 events for customer cus_042 (dispute has no customer)")
    passed += 1

    # ── Step 5: Verify payload preserved ──

    print("\n─── Step 5: Verify payload preservation ───\n")

    payment_events = json.loads(ch.query(source="stripe", event_type="payment_intent.succeeded"))
    payload = payment_events[0]["event"]["payload"]
    assert_eq(payload["amount"], 4999, "payload.amount preserved")
    assert_eq(payload["currency"], "usd", "payload.currency preserved")
    assert_eq(payload["status"], "succeeded", "payload.status preserved")
    passed += 3

    # ── Step 6: Batch ingestion ──

    print("\n─── Step 6: Batch ingestion ───\n")

    batch_ids = ch.ingest_stripe_batch([PAYMENT_SUCCEEDED, CHARGE_FAILED])
    assert_eq(len(batch_ids), 2, "batch returns 2 event IDs")
    passed += 1

    # ── Summary ──

    print("\n" + "=" * 60)
    print(f"  {passed}/{passed} tests passed")
    print("  Stripe webhooks → Chronicle events in one line.")
    print("  Entity refs, topics, and payloads extracted automatically.")
    print("=" * 60 + "\n")


if __name__ == "__main__":
    main()
