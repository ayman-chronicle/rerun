"""
Chronicle Subscription Validation
===================================

Generates events across multiple sources and validates that subscription
callbacks fire correctly for each filter type. Acts as an end-to-end
integration test for the Python subscription API.

Usage:
    python examples/subscription_validation.py
"""

import json
import sys
import chronicle_py as chronicle


class CallbackTracker:
    """Tracks callback invocations for assertion."""

    def __init__(self, name):
        self.name = name
        self.events = []

    def __call__(self, event):
        self.events.append(event)

    def count(self):
        return len(self.events)

    def event_types(self):
        return [e["event_type"] for e in self.events]

    def sources(self):
        return [e["source"] for e in self.events]

    def reset(self):
        self.events.clear()


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
    total = 0

    print("=" * 60)
    print("  Chronicle Subscription Validation")
    print("=" * 60)

    # -----------------------------------------------------------------
    # Test 1: Unfiltered subscription receives all events
    # -----------------------------------------------------------------
    print("\n--- Test 1: Unfiltered subscription receives all events ---")
    total += 1

    all_tracker = CallbackTracker("all")
    handle = ch.subscribe(all_tracker)

    ch.log("stripe", "payments", "charge.created",
           entities={"customer": "c1"}, payload={"amount": 100})
    ch.log("support", "tickets", "ticket.created",
           entities={"customer": "c1"}, payload={"subject": "Help"})
    ch.log("billing", "invoices", "invoice.paid",
           entities={"customer": "c2"}, payload={"amount": 200})

    assert_eq(all_tracker.count(), 3, "unfiltered gets all 3 events")
    handle.cancel()
    passed += 1

    # -----------------------------------------------------------------
    # Test 2: Source filter
    # -----------------------------------------------------------------
    print("\n--- Test 2: Source filter ---")
    total += 1

    stripe_tracker = CallbackTracker("stripe_only")
    handle = ch.subscribe(stripe_tracker, sources=["stripe"])

    ch.log("stripe", "payments", "charge.created",
           entities={"customer": "c3"}, payload={"amount": 300})
    ch.log("support", "tickets", "ticket.created",
           entities={"customer": "c3"}, payload={"subject": "Bug"})
    ch.log("stripe", "payments", "charge.refunded",
           entities={"customer": "c3"}, payload={"amount": 150})

    assert_eq(stripe_tracker.count(), 2, "source filter: 2 stripe events")
    assert_eq(
        set(stripe_tracker.event_types()),
        {"charge.created", "charge.refunded"},
        "source filter: correct event types",
    )
    handle.cancel()
    passed += 1

    # -----------------------------------------------------------------
    # Test 3: Event type filter
    # -----------------------------------------------------------------
    print("\n--- Test 3: Event type filter ---")
    total += 1

    ticket_tracker = CallbackTracker("tickets_only")
    handle = ch.subscribe(ticket_tracker, event_types=["ticket.created"])

    ch.log("support", "tickets", "ticket.created",
           entities={"customer": "c4"}, payload={"subject": "Q1"})
    ch.log("support", "tickets", "ticket.updated",
           entities={"customer": "c4"}, payload={"status": "open"})
    ch.log("support", "tickets", "ticket.created",
           entities={"customer": "c5"}, payload={"subject": "Q2"})

    assert_eq(ticket_tracker.count(), 2, "type filter: 2 ticket.created events")
    handle.cancel()
    passed += 1

    # -----------------------------------------------------------------
    # Test 4: Entity filter (cross-source)
    # -----------------------------------------------------------------
    print("\n--- Test 4: Entity filter (cross-source) ---")
    total += 1

    entity_tracker = CallbackTracker("customer_c6")
    handle = ch.subscribe(entity_tracker, entity_type="customer", entity_id="c6")

    ch.log("stripe", "payments", "charge.created",
           entities={"customer": "c6"}, payload={"amount": 500})
    ch.log("support", "tickets", "ticket.created",
           entities={"customer": "c6"}, payload={"subject": "Issue"})
    ch.log("stripe", "payments", "charge.created",
           entities={"customer": "c7"}, payload={"amount": 600})
    ch.log("billing", "invoices", "invoice.overdue",
           entities={"customer": "c6", "account": "acc_1"}, payload={"days": 7})

    assert_eq(entity_tracker.count(), 3, "entity filter: 3 events for customer c6")
    assert_eq(
        set(entity_tracker.sources()),
        {"stripe", "support", "billing"},
        "entity filter: events from 3 different sources",
    )
    handle.cancel()
    passed += 1

    # -----------------------------------------------------------------
    # Test 5: Payload contains filter
    # -----------------------------------------------------------------
    print("\n--- Test 5: Payload contains filter ---")
    total += 1

    payload_tracker = CallbackTracker("card_declined")
    handle = ch.subscribe(payload_tracker, payload_contains="card_declined")

    ch.log("stripe", "payments", "payment.failed",
           entities={"customer": "c8"},
           payload={"failure_code": "card_declined", "amount": 999})
    ch.log("stripe", "payments", "payment.succeeded",
           entities={"customer": "c8"},
           payload={"status": "ok", "amount": 999})
    ch.log("stripe", "payments", "payment.failed",
           entities={"customer": "c9"},
           payload={"failure_code": "card_declined", "amount": 100})

    assert_eq(payload_tracker.count(), 2, "payload filter: 2 card_declined events")
    handle.cancel()
    passed += 1

    # -----------------------------------------------------------------
    # Test 6: Combined filters (source + entity)
    # -----------------------------------------------------------------
    print("\n--- Test 6: Combined filters (source + entity) ---")
    total += 1

    combined_tracker = CallbackTracker("stripe_c10")
    handle = ch.subscribe(
        combined_tracker,
        sources=["stripe"],
        entity_type="customer",
        entity_id="c10",
    )

    ch.log("stripe", "payments", "charge.created",
           entities={"customer": "c10"}, payload={"amount": 100})
    ch.log("support", "tickets", "ticket.created",
           entities={"customer": "c10"}, payload={"subject": "Q"})
    ch.log("stripe", "payments", "charge.created",
           entities={"customer": "c11"}, payload={"amount": 200})

    assert_eq(combined_tracker.count(), 1, "combined filter: only stripe + c10")
    assert_eq(
        combined_tracker.events[0]["event_type"],
        "charge.created",
        "combined filter: correct event type",
    )
    handle.cancel()
    passed += 1

    # -----------------------------------------------------------------
    # Test 7: Multiple concurrent subscriptions
    # -----------------------------------------------------------------
    print("\n--- Test 7: Multiple concurrent subscriptions ---")
    total += 1

    t1 = CallbackTracker("sub1_stripe")
    t2 = CallbackTracker("sub2_support")
    t3 = CallbackTracker("sub3_all")

    h1 = ch.subscribe(t1, sources=["stripe"])
    h2 = ch.subscribe(t2, sources=["support"])
    h3 = ch.subscribe(t3)

    ch.log("stripe", "payments", "charge.created",
           entities={"customer": "c12"}, payload={"amount": 100})
    ch.log("support", "tickets", "ticket.created",
           entities={"customer": "c12"}, payload={"subject": "X"})

    assert_eq(t1.count(), 1, "concurrent: stripe sub gets 1")
    assert_eq(t2.count(), 1, "concurrent: support sub gets 1")
    assert_eq(t3.count(), 2, "concurrent: unfiltered sub gets 2")

    h1.cancel()
    h2.cancel()
    h3.cancel()
    passed += 1

    # -----------------------------------------------------------------
    # Test 8: Callback receives correct event data
    # -----------------------------------------------------------------
    print("\n--- Test 8: Callback receives correct event data ---")
    total += 1

    data_tracker = CallbackTracker("data_check")
    handle = ch.subscribe(data_tracker, sources=["stripe"])

    ch.log("stripe", "payments", "charge.created",
           entities={"customer": "c13"},
           payload={"amount": 4999, "currency": "usd"})

    evt = data_tracker.events[0]
    assert_eq(evt["source"], "stripe", "event data: source")
    assert_eq(evt["event_type"], "charge.created", "event data: event_type")
    assert_eq(evt["topic"], "payments", "event data: topic")

    payload = evt.get("payload")
    assert_eq(payload is not None, True, "event data: payload exists")
    if payload:
        assert_eq(payload.get("amount"), 4999, "event data: payload.amount")
        assert_eq(payload.get("currency"), "usd", "event data: payload.currency")

    handle.cancel()
    passed += 1

    # -----------------------------------------------------------------
    # Summary
    # -----------------------------------------------------------------
    print("\n" + "=" * 60)
    print(f"  Results: {passed}/{total} tests passed")
    if passed == total:
        print("  All subscription validations passed!")
    else:
        print(f"  {total - passed} tests FAILED")
        sys.exit(1)
    print("=" * 60 + "\n")


if __name__ == "__main__":
    main()
