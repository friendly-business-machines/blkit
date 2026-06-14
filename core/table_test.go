package core

import "testing"

const rates = `tableFromDicts([{region: "domestic", rate: 5.99}, {region: "europe", rate: 15.99}, {region: "intl", rate: 24.50}])`

func TestTable(t *testing.T) {
	assertEval(t, map[string]string{
		// construction + columnar form
		`table(["a", "b"], [1, 2], [3, 4]).nRows`: "2",
		`tableFromDicts([{a: 1}, {a: 2}]).nRows`:  "2",
		// attributes
		rates + `.nRows`:    "3",
		rates + `.nCols`:    "2",
		rates + `.colNames`: `["rate", "region"]`,
		// column projection
		rates + `.region`: `["domestic", "europe", "intl"]`,
		rates + `.rate`:   "[5.99, 15.99, 24.5]",
		// hasColumn
		`hasColumn(` + rates + `, "rate")`:    "true",
		`hasColumn(` + rates + `, "missing")`: "false",
		// indexing → 1-row sub-table; unwrap
		rates + `[1].toDict()`: `{rate: 5.99, region: "domestic"}`,
		rates + `[-1].region`:  `["intl"]`,
		rates + `.rate[1]`:     "5.99",
		// row filter (column scope)
		`(` + rates + `[rate > 10]).nRows`:  "2",
		`(` + rates + `[rate > 10]).region`: `["europe", "intl"]`,
		// methods
		`(` + rates + `.filter(rate > 10)).nRows`:                                        "2",
		`(` + rates + `.filterOut(rate > 10)).nRows`:                                     "1",
		`(` + rates + `.select("region")).colNames`:                                      `["region"]`,
		`(` + rates + `.rename("rate", "price")).colNames`:                               `["price", "region"]`,
		`(` + rates + `.sort(desc("rate"))).rate`:                                        "[24.5, 15.99, 5.99]",
		`(` + rates + `.sort("rate")).rate`:                                              "[5.99, 15.99, 24.5]",
		`(` + rates + `.sort(asc("rate"))).rate`:                                         "[5.99, 15.99, 24.5]",
		`(` + rates + `.sort(inOrder("region", ["europe", "domestic", "intl"]))).region`: `["europe", "domestic", "intl"]`,
		`(` + rates + `.withColumn("withTax", rate * 2)).withTax`:                        "[11.98, 31.98, 49]",
		`(` + rates + `.distinct()).nRows`:                                               "3",
		`(` + rates + `.slice([1, 3])).region`:                                           `["domestic", "intl"]`,
		// aggregates over a column / table
		`sum(` + rates + `.rate)`: "46.48",
		`count(` + rates + `)`:    "3",
		`max(` + rates + `.rate)`: "24.5",
		// for/some/every over table rows
		`sum(for r in ` + rates + ` return r.rate)`:     "46.48",
		`some r in ` + rates + ` satisfies r.rate > 20`: "true",
		`every r in ` + rates + ` satisfies r.rate > 0`: "true",
		// groupBy / agg
		`(tableFromDicts([{g: "a", v: 1}, {g: "a", v: 2}, {g: "b", v: 10}]).groupBy("g").agg("total", sum(v), "n", count(item))).total`: "[3, 10]",
		// union (stacking)
		`union(tableFromDicts([{a: 1}]), tableFromDicts([{a: 2}])).nRows`: "2",
	})
}

func TestTableJoin(t *testing.T) {
	expr := `join(tableFromDicts([{id: 1, region: "NA"}, {id: 2, region: "EU"}]), tableFromDicts([{id: 1, amt: 100}, {id: 2, amt: 200}]), "id").nRows`
	if got := evalNil(t, expr); got != "2" {
		t.Errorf("inner join nRows = %q, want 2", got)
	}
}

func TestTableTwoSlotBracket(t *testing.T) {
	tab := `tableFromDicts([{region: "domestic", rate: 5.99}, {region: "europe", rate: 15.99}])`
	assertEval(t, map[string]string{
		tab + `[1, "rate"].toValue()`:               "5.99",
		tab + `[1, "region"].toValue()`:             "domestic",
		`(` + tab + `[, "region"]).colNames`:        `["region"]`,
		`(` + tab + `[, "region"]).nRows`:           "2",
		`(` + tab + `[, ["region", "rate"]]).nCols`: "2",
		tab + `[1, ["region", "rate"]].nCols`:       "2",
		`(` + tab + `[[1, 2], "rate"]).nRows`:       "2",
	})
}
