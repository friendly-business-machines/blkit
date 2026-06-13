package blkit

// componentAccessFn is the single runtime-dispatching accessor the patcher emits
// for every dot-component access (x.year, d.minutes, dict.key, …). It switches
// on the receiver's runtime type because the correct accessor depends on the
// operand type, which isn't reliably known at patch time.
func componentAccessFn(args ...any) (any, error) {
	recv := asBl(args[0])
	nameVal, ok := args[1].(BlString)
	if !ok {
		return nil, argTypeError(args[1])
	}
	name := nameVal.s
	if recv.IsNull() {
		return Null(), nil
	}
	switch v := recv.(type) {
	case BlDictionary:
		if val, present := v.get(name); present {
			return val, nil
		}
		return Null(), nil
	case BlDate:
		if r, ok := dateComponent(v, name); ok {
			return r, nil
		}
	case BlTime:
		if r, ok := timeComponent(v, name); ok {
			return r, nil
		}
	case BlDateTime:
		if r, ok := datetimeComponent(v, name); ok {
			return r, nil
		}
	case BlDaysTimeDuration:
		if r, ok := dtDurationComponent(v, name); ok {
			return r, nil
		}
	case BlYearsMonthsDuration:
		if r, ok := ymDurationComponent(v, name); ok {
			return r, nil
		}
	case BlRange:
		if r, ok := rangeComponent(v, name); ok {
			return r, nil
		}
	case BlTable:
		if r, ok := tableComponent(v, name); ok {
			return r, nil
		}
	case BlList:
		// Field access on a list of dictionaries projects the field across
		// every element (sugar for `for x in list return x.field`).
		out := make([]BlValue, len(v.items))
		for i, e := range v.items {
			if e.IsNull() {
				out[i] = Null()
				continue
			}
			ev, err := componentAccessFn(e, nameVal)
			if err != nil {
				return nil, err
			}
			out[i] = asBl(ev)
		}
		return BlList{out}, nil
	}
	return Null(), nil
}

func dtDurationComponent(d BlDaysTimeDuration, name string) (BlValue, bool) {
	switch name {
	case "days":
		return durationDaysDTFn(d), true
	case "hours":
		return durationHoursDTFn(d), true
	case "minutes":
		return durationMinutesDTFn(d), true
	case "seconds":
		return durationSecondsDTFn(d), true
	case "totalSeconds":
		return durationTotalSecondsFn(d), true
	case "totalMinutes":
		return durationTotalMinutesFn(d), true
	case "totalHours":
		return durationTotalHoursFn(d), true
	case "totalDays":
		return durationTotalDaysFn(d), true
	}
	return nil, false
}

func ymDurationComponent(d BlYearsMonthsDuration, name string) (BlValue, bool) {
	switch name {
	case "years":
		return durationYearsYMFn(d), true
	case "months":
		return durationMonthsYMFn(d), true
	case "totalMonths":
		return durationTotalMonthsFn(d), true
	case "totalYears":
		return durationTotalYearsFn(d), true
	}
	return nil, false
}
