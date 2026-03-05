"""
Chronicle Gorgias Connector Demo
==================================

Demonstrates first-class Gorgias support: webhook ingestion with
automatic entity ref extraction, topic derivation, and the generic
ch.ingest() routing.

Usage:
    python examples/gorgias_connector_demo.py
"""

import json
import sys
import chronicle_py as chronicle


TICKET_CREATED = json.dumps({
    "id": 100, "type": "ticket-created", "object_id": 500, "object_type": "Ticket",
    "created_datetime": "2024-03-15T10:30:00.000000", "user_id": 42,
    "context": "dd4ff312-69df-494a-be96-1a58b3d8b8e0",
    "data": {"customer_id": 777, "subject": "Where is my order?"}
})

TICKET_CLOSED = json.dumps({
    "id": 101, "type": "ticket-closed", "object_id": 500, "object_type": "Ticket",
    "created_datetime": "2024-03-15T14:00:00.000000", "user_id": 42,
    "data": {}
})

MESSAGE_CREATED = json.dumps({
    "id": 200, "type": "ticket-message-created", "object_id": 8000, "object_type": "TicketMessage",
    "created_datetime": "2024-03-15T10:35:00.000000", "user_id": None,
    "data": {"ticket_id": 500, "customer_id": 777, "body_text": "I ordered 3 days ago and haven't received it."}
})

CUSTOMER_CREATED = json.dumps({
    "id": 300, "type": "customer-created", "object_id": 777, "object_type": "Customer",
    "created_datetime": "2024-03-14T09:00:00.000000", "user_id": None,
    "data": {"email": "john@example.com", "name": "John Smith"}
})

CSAT_RESPONDED = json.dumps({
    "id": 400, "type": "satisfaction-survey-responded", "object_id": 50, "object_type": "SatisfactionSurvey",
    "created_datetime": "2024-03-16T14:00:00.000000", "user_id": None,
    "data": {"ticket_id": 500, "customer_id": 777, "score": 5}
})

STRIPE_PAYMENT = json.dumps({
    "id": "evt_pay1", "type": "payment_intent.succeeded", "created": 1710489600,
    "data": {"object": {"id": "pi_abc", "object": "payment_intent", "amount": 2999, "currency": "usd", "customer": "cus_777"}}
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
    print("  Chronicle Connector Demo: Gorgias + Stripe + Generic")
    print("=" * 60)

    # ── Gorgias source-specific ingestion ──

    print("\n--- Gorgias: source-specific ingestion ---\n")

    id1 = ch.ingest_gorgias(TICKET_CREATED)
    print(f"  ticket-created         → {id1[:16]}…")
    id2 = ch.ingest_gorgias(MESSAGE_CREATED)
    print(f"  ticket-message-created → {id2[:16]}…")
    id3 = ch.ingest_gorgias(CUSTOMER_CREATED)
    print(f"  customer-created       → {id3[:16]}…")
    id4 = ch.ingest_gorgias(TICKET_CLOSED)
    print(f"  ticket-closed          → {id4[:16]}…")
    id5 = ch.ingest_gorgias(CSAT_RESPONDED)
    print(f"  satisfaction-survey     → {id5[:16]}…")

    gorgias_events = json.loads(ch.query(source="gorgias"))
    assert_eq(len(gorgias_events), 5, "5 Gorgias events ingested")
    passed += 1

    # ── Verify topics ──

    print("\n--- Verify topic auto-derivation ---\n")

    topics = {e["event"]["topic"] for e in gorgias_events}
    assert_eq("tickets" in topics, True, "tickets topic")
    assert_eq("messages" in topics, True, "messages topic")
    assert_eq("customers" in topics, True, "customers topic")
    assert_eq("csat" in topics, True, "csat topic")
    passed += 4

    # ── Generic routing ──

    print("\n--- Generic ch.ingest() routing ---\n")

    id_stripe = ch.ingest("stripe", STRIPE_PAYMENT)
    print(f"  ingest('stripe', ...) → {id_stripe[:16]}…")

    id_gorgias = ch.ingest("gorgias", TICKET_CREATED)
    print(f"  ingest('gorgias', ..) → {id_gorgias[:16]}…")

    all_events = json.loads(ch.query())
    stripe_count = sum(1 for e in all_events if e["event"]["source"] == "stripe")
    gorgias_count = sum(1 for e in all_events if e["event"]["source"] == "gorgias")
    assert_eq(stripe_count, 1, "generic ingest routed to stripe")
    assert_eq(gorgias_count, 6, "generic ingest routed to gorgias (5 + 1)")
    passed += 2

    # ── Unknown source error ──

    print("\n--- Unknown source error ---\n")

    try:
        ch.ingest("shopify", "{}")
        print("  FAIL: should have raised error")
        sys.exit(1)
    except ValueError as e:
        assert_eq("Unknown source" in str(e), True, "unknown source raises ValueError")
        passed += 1

    # ── Batch ingestion ──

    print("\n--- Batch Gorgias ingestion ---\n")

    batch_ids = ch.ingest_gorgias_batch([TICKET_CREATED, MESSAGE_CREATED])
    assert_eq(len(batch_ids), 2, "batch returns 2 event IDs")
    passed += 1

    # ── Summary ──

    print("\n" + "=" * 60)
    print(f"  {passed}/{passed} tests passed")
    print("  Gorgias + Stripe connectors work via trait + generic ingest.")
    print("=" * 60 + "\n")


if __name__ == "__main__":
    main()
