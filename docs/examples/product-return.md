# Product Return and Refund

> A customer requests a return; the process validates eligibility within the
> return window, grades the item's condition on receipt, and issues a refund,
> replacement, or store credit accordingly.

## Business overview

A customer may request to return a purchased product within the return window.
The process validates the request, determines whether the item is eligible,
assesses the item's condition on receipt, and then issues a refund, a
replacement, or a store credit depending on the outcome. Two time boundaries
apply:

- **Return request window** — the customer must submit the return request within
  30 calendar days of the delivery date.
- **Item receipt window** — once a return is authorised, the physical item must be
  received at the warehouse within 14 calendar days of the authorisation date. If
  not, the authorisation expires and no refund is issued.

### Return request information

| Field | Description |
|---|---|
| Order Number | The original purchase order reference |
| Customer ID | Unique identifier of the requesting customer |
| Product Code | SKU of the item being returned |
| Quantity | Number of units being returned |
| Delivery Date | Date the original order was delivered |
| Return Request Date | Date the customer submitted the return request |
| Return Reason | Reason selected from the standard reason list |
| Preferred Resolution | Refund, replacement, or store credit |

#### Standard return reasons

| Reason Code | Description |
|---|---|
| DEFECTIVE | Item arrived damaged or does not function as advertised |
| WRONG_ITEM | Incorrect product was delivered |
| NOT_AS_DESCRIBED | Item does not match the product listing |
| NO_LONGER_NEEDED | Customer changed their mind |
| DUPLICATE_ORDER | Customer accidentally ordered twice |

### Eligibility rules

A return is eligible only if **all** of the following hold:

| Rule | Condition | Result if false |
|---|---|---|
| E1 | Return request date is within 30 calendar days of delivery date | Rejected: outside return window |
| E2 | Product is flagged as returnable in the catalogue | Rejected: non-returnable item |
| E3 | Quantity requested does not exceed the quantity originally purchased | Rejected: quantity exceeds original purchase |

Non-returnable categories: perishable food, personalised items, downloadable
software, and hazardous materials.

### Return authorisation

If all eligibility rules pass, the process issues a Return Merchandise
Authorisation (RMA) number and sends a prepaid return label. The 14-calendar-day
receipt window begins from the RMA issue date.

### Item condition assessment

When the item arrives, a returns operative inspects it and records a grade.

| Grade | Description |
|---|---|
| A – Unopened | Item in original sealed packaging, indistinguishable from new |
| B – Like new | Item opened but unused, all original packaging and accessories present |
| C – Used | Item shows signs of use; packaging may be missing or damaged |
| D – Damaged | Item is broken, scratched, or otherwise impaired beyond normal use |

### Refund calculation rules

The refund percentage depends on the return reason and the condition grade.

| Return Reason | Condition Grade | Refund Percentage |
|---|---|---|
| DEFECTIVE | Any | 100% |
| WRONG_ITEM | Any | 100% |
| NOT_AS_DESCRIBED | Any | 100% |
| NO_LONGER_NEEDED | A – Unopened | 100% |
| NO_LONGER_NEEDED | B – Like new | 100% |
| NO_LONGER_NEEDED | C – Used | 50% |
| NO_LONGER_NEEDED | D – Damaged | 0% |
| DUPLICATE_ORDER | A – Unopened | 100% |
| DUPLICATE_ORDER | B – Like new | 100% |
| DUPLICATE_ORDER | C – Used | 50% |
| DUPLICATE_ORDER | D – Damaged | 0% |

> Refund = Unit Price × Quantity Returned × Refund Percentage

### Resolution rules

The resolution issued depends on the refund percentage, whether a replacement is
in stock, and the customer's preferred resolution.

| Refund Percentage | Replacement in Stock | Preferred Resolution | Resolution Issued |
|---|---|---|---|
| 100% | Yes | Replacement | Replacement dispatched, no charge |
| 100% | No | Replacement | Full refund to original payment method |
| 100% | Yes or No | Refund | Full refund to original payment method |
| 100% | Yes or No | Store credit | Store credit at 110% of refund amount |
| 50% | Yes | Replacement | Replacement dispatched; 50% of unit price charged |
| 50% | No | Replacement | Partial refund at 50% to original payment method |
| 50% | Yes or No | Refund | Partial refund at 50% to original payment method |
| 50% | Yes or No | Store credit | Store credit at 50% of unit price |
| 0% | Any | Any | Request declined; item returned to customer or disposed of if declined |

### Outcomes

| Condition | Outcome |
|---|---|
| Request outside 30-day window | Rejected; no RMA issued |
| Product is non-returnable | Rejected; no RMA issued |
| Quantity exceeds original purchase | Rejected; no RMA issued |
| RMA issued but item not received within 14 days | Authorisation expired; no refund |
| Refund 100%, wants replacement, stock available | Replacement dispatched |
| Refund 100%, replacement unavailable or wants refund | Full monetary refund |
| Refund 100%, wants store credit | Store credit at 110% of purchase price |
| Refund 50%, various | Partial refund or partial store credit |
| Refund 0% | Declined; item returned to customer |

### Worked examples

| Order | Product | Unit Price | Qty | Return Reason | Condition | Preferred | Stock | Refund % | Refund Amount | Resolution |
|---|---|---|---|---|---|---|---|---|---|---|
| ORD-101 | Headphones | £80 | 1 | DEFECTIVE | D – Damaged | Replacement | Yes | 100% | £80 | Replacement dispatched |
| ORD-102 | Notebook | £12 | 3 | NO_LONGER_NEEDED | C – Used | Refund | — | 50% | £18 | Partial refund £18 |
| ORD-103 | Desk Chair | £350 | 1 | NOT_AS_DESCRIBED | B – Like new | Store credit | — | 100% | £350 | Store credit £385 |
| ORD-104 | USB Cable | £8 | 2 | DUPLICATE_ORDER | A – Unopened | Refund | — | 100% | £16 | Full refund £16 |
| ORD-105 | Monitor | £450 | 1 | NO_LONGER_NEEDED | D – Damaged | Refund | — | 0% | £0 | Declined |

For ORD-103, the return is submitted on day 12 (within 30 days) for a returnable
item, quantity 1 of 1 — eligible, RMA issued. The chair arrives on day 8 of the
14-day window and is graded B. NOT_AS_DESCRIBED with any grade → 100%, so refund
= £350 × 1 × 100% = £350. The customer's preferred resolution is store credit,
issued at 110% = **£385**.

## Implementation

The eligibility, refund, and resolution logic can already be composed as one
typed `DecisionExpression`.

``` { .go .blkit-example title="main.go" }
package main

import (
	"encoding/json"
	"fmt"
	bl "github.com/friendly-business-machines/blkit/core"
	"os"
)

type ReturnInput struct {
	DeliveryDate       string `json:"delivery_date"`
	RequestDate        string `json:"request_date"`
	RMAIssuedDate      string `json:"rma_issued_date"`
	ReceivedDate       string `json:"received_date"`
	Returnable         bool   `json:"returnable"`
	Quantity           int    `json:"quantity"`
	Purchased          int    `json:"purchased"`
	Reason             string `json:"reason"`
	Grade              string `json:"grade"`
	Preferred          string `json:"preferred"`
	ReplacementInStock bool   `json:"replacement_in_stock"`
	UnitPrice          string `json:"unit_price"`
}
type ReturnResult struct {
	Eligible         bool   `json:"eligible"`
	ReceivedInTime   bool   `json:"received_in_time"`
	RefundPercent    string `json:"refund_percent"`
	RefundAmount     string `json:"refund_amount"`
	Resolution       string `json:"resolution"`
	ResolutionAmount string `json:"resolution_amount"`
}
type returnVars struct {
	Delivery   bl.Handle[bl.BlDate]    `expr:"delivery"`
	Request    bl.Handle[bl.BlDate]    `expr:"request"`
	Issued     bl.Handle[bl.BlDate]    `expr:"issued"`
	Received   bl.Handle[bl.BlDate]    `expr:"received"`
	Returnable bl.Handle[bl.BlBoolean] `expr:"returnable"`
	Quantity   bl.Handle[bl.BlNumber]  `expr:"quantity"`
	Purchased  bl.Handle[bl.BlNumber]  `expr:"purchased"`
	Reason     bl.Handle[bl.BlString]  `expr:"reason"`
	Grade      bl.Handle[bl.BlString]  `expr:"grade"`
	Preferred  bl.Handle[bl.BlString]  `expr:"preferred"`
	Stock      bl.Handle[bl.BlBoolean] `expr:"stock"`
	Price      bl.Handle[bl.BlNumber]  `expr:"price"`
}
type returnOutputs struct {
	WithinRequestWindow bl.Handle[bl.BlBoolean] `expr:"within_request_window"`
	ReceivedInTime      bl.Handle[bl.BlBoolean] `expr:"received_in_time"`
	Eligible            bl.Handle[bl.BlBoolean] `expr:"eligible"`
	RefundPercent       bl.Handle[bl.BlNumber]  `expr:"refund_percent"`
	RefundAmount        bl.Handle[bl.BlNumber]  `expr:"refund_amount"`
	ResolutionAmount    bl.Handle[bl.BlNumber]  `expr:"resolution_amount"`
	Resolution          bl.Handle[bl.BlString]  `expr:"resolution"`
}
```

Sequential entries make each decision visible and reusable by later entries.

``` { .go .blkit-example title="main.go" }
var returnDecision = bl.NewDecisionExpression[returnVars, returnOutputs](bl.DecisionExpressionConfig{
	Id: "product-return",
	Entries: bl.Entries{
		"within_request_window": `request-delivery <= dtDuration("P30D")`,
		"received_in_time":      `received-issued <= dtDuration("P14D")`,
		"eligible":              `within_request_window and returnable and quantity <= purchased`,
		"refund_percent":        `if not(eligible) or not(received_in_time) then 0 else if reason in ["DEFECTIVE","WRONG_ITEM","NOT_AS_DESCRIBED"] then 100 else if grade in ["A","B"] then 100 else if grade = "C" then 50 else 0`,
		"refund_amount":         `price*quantity*refund_percent/100`,
		"resolution_amount":     `if preferred="store_credit" and refund_percent=100 then refund_amount*1.1 else refund_amount`,
		"resolution":            `if refund_percent=0 then "Declined" else if preferred="replacement" and stock then "Replacement dispatched" else if preferred="store_credit" then "Store credit" else if refund_percent=100 then "Full refund" else "Partial refund"`,
	},
})

func CalculateReturn(in ReturnInput) (ReturnResult, error) {
	delivery, e := bl.Date(in.DeliveryDate)
	if e != nil {
		return ReturnResult{}, e
	}
	request, e := bl.Date(in.RequestDate)
	if e != nil {
		return ReturnResult{}, e
	}
	issued, e := bl.Date(in.RMAIssuedDate)
	if e != nil {
		return ReturnResult{}, e
	}
	received, e := bl.Date(in.ReceivedDate)
	if e != nil {
		return ReturnResult{}, e
	}
	q, _ := bl.Number(in.Quantity)
	p, _ := bl.Number(in.Purchased)
	reason, _ := bl.String(in.Reason)
	grade, _ := bl.String(in.Grade)
	preferred, _ := bl.String(in.Preferred)
	price, e := bl.Number(in.UnitPrice)
	if e != nil {
		return ReturnResult{}, e
	}
	returnable, _ := bl.Boolean(in.Returnable)
	stock, _ := bl.Boolean(in.ReplacementInStock)
	v, e := returnDecision.Evaluate(returnVars{
		bl.NewHandle(delivery), bl.NewHandle(request), bl.NewHandle(issued),
		bl.NewHandle(received), bl.NewHandle(returnable), bl.NewHandle(q),
		bl.NewHandle(p), bl.NewHandle(reason), bl.NewHandle(grade),
		bl.NewHandle(preferred), bl.NewHandle(stock), bl.NewHandle(price),
	})
	if e != nil {
		return ReturnResult{}, e
	}
	return ReturnResult{
		v.Eligible.Get().Native(), v.ReceivedInTime.Get().Native(),
		v.RefundPercent.Get().String(), v.RefundAmount.Get().String(),
		v.Resolution.Get().String(), v.ResolutionAmount.Get().String(),
	}, nil
}
```

``` { .go .blkit-example title="main.go" }
func main() {
	var in ReturnInput
	if e := json.NewDecoder(os.Stdin).Decode(&in); e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
	out, e := CalculateReturn(in)
	if e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
	if e = json.NewEncoder(os.Stdout).Encode(out); e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
}
```

RMA creation, waiting for warehouse receipt, timer expiry, and dispatch/payment
side effects require the unfinished process engine, so no Go source for those
steps is shown yet.

## Notes

- Two distinct timers govern the flow: a 30-calendar-day eligibility window
  before authorisation, and a 14-calendar-day receipt window after it — both
  natural fits for blkit's date and duration expressions.
- Refund percentage and resolution are separate named decision outputs: the
  percentage depends on reason and grade, then feeds the resolution calculation
  alongside stock and preference.
