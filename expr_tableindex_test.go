package blkit

import "testing"

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
