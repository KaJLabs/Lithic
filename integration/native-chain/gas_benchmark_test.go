//go:build lithovm_chain_lab

package nativechain

import (
	"encoding/hex"
	"fmt"
	"strings"
	"testing"

	storetypes "cosmossdk.io/store/types"
	"github.com/cosmos/cosmos-sdk/testutil"
	"github.com/ethereum/go-ethereum/common"
	"github.com/evmos/evmos/v20/x/evm/statedb"
)

func mapArgs(account, amount uint64) string {
	return fmt.Sprintf("0x4c56414c01000204%064x02%064x", account, amount)
}

// ExecuteFrame + actual Rust FFI/SDK cached state, not an arithmetic microbench.
// Compiler, fixture population, fresh StateDB construction and reset are outside
// the timer. This does NOT measure disk commits, node RPC or validator consensus.
func BenchmarkCandidateGas(b *testing.B) {
	for _, entries := range []int{0, 16, 64} {
		for _, operation := range []string{"read", "overwrite", "grow", "log256", "log4096", "escaped4096"} {
			b.Run(fmt.Sprintf("entries%d/%s", entries, operation), func(b *testing.B) {
				key := storetypes.NewKVStoreKey("benchmark")
				ctx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("benchmark-t"))
				req := request(b)
				req.Bytecode = ptr(fixture(b, "testdata/storage_gas.lithic"))
				req.Arguments = args()
				env := Environment{ChainID: 700777, Height: 1, Timestamp: 2, Nonce: 1}
				db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
				deployed, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
				if err != nil {
					b.Fatal(err)
				}
				if err := db.Commit(); err != nil {
					b.Fatal(err)
				}
				contract := returnedAddress(b, deployed)
				req.Operation = "call"
				req.Bytecode = nil
				req.Contract = &contract
				req.Function = ptr("write")
				for i := 0; i < entries; i++ {
					db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
					req.Arguments = mapArgs(uint64(i+1), 1)
					if _, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req); err != nil {
						b.Fatal(err)
					}
					if err := db.Commit(); err != nil {
						b.Fatal(err)
					}
				}
				switch operation {
				case "read":
					req.Function = ptr("read")
					req.Arguments = fmt.Sprintf("0x4c56414c01000104%064x", 1)
				case "overwrite":
					req.Arguments = mapArgs(1, 2)
				case "grow":
					req.Arguments = mapArgs(uint64(entries+1), 2)
				default:
					text := strings.Repeat("x", 256)
					if operation == "log4096" {
						text = strings.Repeat("x", 4096)
					}
					if operation == "escaped4096" {
						text = strings.Repeat("\x00", 4096)
					}
					req.Function = ptr("note")
					req.Arguments = fmt.Sprintf("0x4c56414c01000106%04x%s", len(text), hex.EncodeToString([]byte(text)))
				}
				record := ctx.KVStore(key).Get([]byte("lithovm/v1/contracts/" + strings.TrimPrefix(contract, "0x")))
				var used uint64
				b.ReportAllocs()
				b.ResetTimer()
				for i := 0; i < b.N; i++ {
					b.StopTimer()
					db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
					f := frame(10_000_000)
					b.StartTimer()
					result, err := ExecuteFrame(db, key, f, false, env, req)
					b.StopTimer()
					if err != nil || !result.Success {
						b.Fatalf("%v %+v", err, result)
					}
					cost := uint64(10_000_000) - f.Gas
					if i > 0 && cost != used {
						b.Fatal("nondeterministic gas")
					}
					used = cost
					b.StartTimer()
				}
				b.StopTimer()
				b.ReportMetric(float64(used), "gas/op")
				b.ReportMetric(float64(len(record)), "record-B")
			})
		}
	}
}
