//go:build lithovm_chain_lab

package nativechain

import (
	"bytes"
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	storetypes "cosmossdk.io/store/types"
	"github.com/cosmos/cosmos-sdk/testutil"
)

func TestNativeGenesisRoundTripAndRejectsPartialImport(t *testing.T) {
	key := storetypes.NewKVStoreKey("native-genesis-source")
	ctx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("native-genesis-source-t"))
	store := ctx.KVStore(key)
	req := request(t)
	result, err := Execute(req, func(string) ([]byte, error) { return nil, nil })
	if err != nil || !result.Success {
		t.Fatalf("deploy fixture: %v %+v", err, result)
	}
	writes, err := result.DecodedWrites()
	if err != nil {
		t.Fatal(err)
	}
	var contractKey string
	for name, value := range writes {
		contractKey = name
		store.Set([]byte(name), value)
	}
	mapKey := "lithovm/v2/maps/" + strings.TrimPrefix(returnedAddress(t, result), "0x") + "/balances/" + fmt.Sprintf("%064x", 7)
	word := make([]byte, 32)
	word[31] = 42
	store.Set([]byte(mapKey), word)
	snapshot, err := ExportNativeGenesis(store)
	if err != nil {
		t.Fatal(err)
	}
	destination := storetypes.NewKVStoreKey("native-genesis-destination")
	destCtx := testutil.DefaultContext(destination, storetypes.NewTransientStoreKey("native-genesis-destination-t"))
	dest := destCtx.KVStore(destination)
	if err := ImportNativeGenesis(dest, snapshot); err != nil {
		t.Fatal(err)
	}
	reexported, err := ExportNativeGenesis(dest)
	if err != nil || !bytes.Equal(reexported, snapshot) || !bytes.Equal(dest.Get([]byte(contractKey)), writes[contractKey]) || !bytes.Equal(dest.Get([]byte(mapKey)), word) {
		t.Fatalf("native genesis round trip mismatch: %v", err)
	}
	if err := ImportNativeGenesis(dest, snapshot); err == nil {
		t.Fatal("nonempty destination accepted import")
	}

	for _, mutate := range []func(*nativeGenesis){
		func(value *nativeGenesis) { value.Entries[0].Value = "0x01" },
		func(value *nativeGenesis) { value.Entries = append(value.Entries, value.Entries[0]) },
		func(value *nativeGenesis) { value.Entries[0].Key = "other/invalid" },
		func(value *nativeGenesis) { value.Entries = value.Entries[1:] },
		func(value *nativeGenesis) { value.Version = 2 },
	} {
		var changed nativeGenesis
		if err := json.Unmarshal(snapshot, &changed); err != nil {
			t.Fatal(err)
		}
		mutate(&changed)
		payload, err := json.Marshal(changed)
		if err != nil {
			t.Fatal(err)
		}
		freshKey := storetypes.NewKVStoreKey("native-genesis-invalid")
		freshCtx := testutil.DefaultContext(freshKey, storetypes.NewTransientStoreKey("native-genesis-invalid-t"))
		fresh := freshCtx.KVStore(freshKey)
		if err := ImportNativeGenesis(fresh, payload); err == nil {
			t.Fatalf("accepted invalid native genesis: %s", payload)
		}
		iterator := fresh.Iterator(nil, nil)
		if iterator.Valid() {
			t.Fatal("invalid import wrote partial native state")
		}
		iterator.Close()
	}
}
