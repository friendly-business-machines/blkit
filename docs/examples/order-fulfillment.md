# Example: Order Fulfillment Process

> An e-commerce platform handles an order from placement through stock
> reservation, payment, picking, dispatch, and delivery — with explicit branches
> for out-of-stock and payment-failure cases.

## Business overview

When a customer places an order on an e-commerce platform, the fulfilment
system must:

1. Validate the order (items exist, quantities make sense, address is complete).
2. Check whether the requested stock is available.
3. If stock is available: reserve inventory, charge payment, pick and pack,
   ship, and send a confirmation.
4. If stock is unavailable: notify the customer of a backorder and end.
5. If payment fails: release the reserved inventory, notify the customer, and
   end in error.

### Input

| Field | Type | Description |
|---|---|---|
| `order_id` | `string` | Unique identifier for the order |
| `customer_id` | `string` | Customer placing the order |
| `items` | `list` | List of `{ sku: string, quantity: number }` objects |
| `payment_method_token` | `string` | Tokenised payment instrument |
| `shipping_address` | `object` | `{ line1, city, postcode, country }` |

### Variables produced during execution

| Variable | Set by | Description |
|---|---|---|
| `validation_errors` | Validate Order | List of validation error messages; empty if valid |
| `stock_available` | Check Stock | `true` or `false` |
| `reservation_id` | Reserve Inventory | Identifier for the inventory reservation |
| `payment_ok` | Charge Payment | `true` or `false` |
| `payment_error` | Charge Payment | Error message if payment failed |
| `shipment_id` | Ship Order | Carrier tracking reference |

### Process graph

```text
StartEvent("start")
  → Activity("validate-order",    "Validate Order")
  → xor("stock-check",            "In stock?",          on: stock_available)
        true  → Activity("reserve-inventory", "Reserve Inventory")
                → Activity("charge-payment",  "Charge Payment")
                → xor("payment-check",        "Payment OK?",   on: payment_ok)
                      true  → Activity("pick-and-pack",  "Pick and Pack")
                              → Activity("ship-order",    "Ship Order")
                              → Activity("send-confirm",  "Send Confirmation")
                              → EndEvent("end-success",   "Order Fulfilled")
                      false → Activity("release-stock",  "Release Inventory")
                              → Activity("notify-payment-failed", "Notify Payment Failure")
                              → EndEvent("end-payment-error", kind=ERROR, "Payment Failed")
        false → Activity("notify-backorder", "Notify Backorder")
                → EndEvent("end-backorder",  "Order Backordered")
```

### Scenarios and expected paths

| Scenario | `stock_available` | `payment_ok` | Terminal node | Description |
|---|---|---|---|---|
| Happy path | `true` | `true` | `end-success` | Order shipped and confirmed |
| Out of stock | `false` | — | `end-backorder` | Customer notified, process ends cleanly |
| Payment declined | `true` | `false` | `end-payment-error` | Stock released, customer notified |

## Implementation

!!! warning "Implementation pending"
    This is a sequential process with two exclusive (XOR) gateways and three
    terminal end events (one of them an error end). The Go implementation
    depends on the `processes` package, which is still being built. This page
    documents the process; the runnable blkit code will be added once that
    package lands.

    In the meantime, see the authoritative
    [business spec](https://github.com/friendly-business-machines/blkit/blob/main/specs/examples/order-fulfillment.spec.md),
    [Getting started](../getting-started/index.md) for orientation, and the
    [Reference](../reference/blkit.md) for the expression engine available today.

## Notes

- The two gateways branch on boolean variables (`stock_available`, `payment_ok`)
  set by earlier activities — illustrating how process flow is driven by data
  produced during execution.
- The payment-failure branch compensates by releasing the inventory reserved
  earlier, then ends via an **error** end event, distinct from the clean
  backorder end.
