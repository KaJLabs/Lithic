//go:build lithovm_bank_candidate

package nativechain

import (
	"encoding/json"
	"fmt"
	"testing"
)

func TestBankBatchRejectsForgeryAliasesAndNonconservation(t *testing.T) {
	good := BankResponse{Response: Response{Success: true}, BankVersion: 1, Balances: []BalanceChange{
		{fmt.Sprintf("0x%040x", 1), fmt.Sprintf("0x%064x", 100), fmt.Sprintf("0x%064x", 93)},
		{fmt.Sprintf("0x%040x", 2), fmt.Sprintf("0x%064x", 0), fmt.Sprintf("0x%064x", 7)},
	}}
	if _, err := good.DecodedBalances(); err != nil {
		t.Fatal(err)
	}
	for _, fault := range []string{"version", "nil", "failed", "mint", "burn", "alias", "duplicate", "unsorted", "unchanged", "limit"} {
		t.Run(fault, func(t *testing.T) {
			raw, _ := json.Marshal(good)
			var r BankResponse
			json.Unmarshal(raw, &r)
			switch fault {
			case "version":
				r.BankVersion = 2
			case "nil":
				r.Balances = nil
			case "failed":
				r.Success = false
			case "mint":
				r.Balances[1].After = fmt.Sprintf("0x%064x", 8)
			case "burn":
				r.Balances[1].After = fmt.Sprintf("0x%064x", 6)
			case "alias":
				r.Balances[0].Address = "0X" + r.Balances[0].Address[2:]
			case "duplicate":
				r.Balances[1].Address = r.Balances[0].Address
			case "unsorted":
				r.Balances[0], r.Balances[1] = r.Balances[1], r.Balances[0]
			case "unchanged":
				r.Balances[0].After = r.Balances[0].Before
			case "limit":
				r.Balances = make([]BalanceChange, 129)
			}
			if _, err := r.DecodedBalances(); err == nil {
				t.Fatal("accepted malformed bank batch")
			}
		})
	}
}
