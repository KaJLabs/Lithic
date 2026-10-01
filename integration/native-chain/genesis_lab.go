//go:build lithovm_chain_lab

package nativechain

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"

	storetypes "cosmossdk.io/store/types"
	"github.com/ethereum/go-ethereum/crypto"
)

// These bounds protect the isolated genesis codec. They are not production
// scalability guarantees; production import/export must be benchmarked.
const nativeGenesisMaxEntries = 1_000_000
const nativeGenesisMaxBytes = 512 * 1024 * 1024

type nativeGenesisEntry struct {
	Key   string `json:"key"`
	Value string `json:"value"`
}

type nativeGenesis struct {
	Version int                  `json:"version"`
	Entries []nativeGenesisEntry `json:"entries"`
}

func validateNativeEntry(key string, value []byte) error {
	if !validStateKey(key) {
		return errors.New("invalid native genesis key")
	}
	if strings.HasPrefix(key, "lithovm/v2/maps/") {
		if len(value) != 32 {
			return errors.New("invalid native map word")
		}
		return nil
	}
	if len(value) == 0 || len(value) > MaxBytes {
		return errors.New("invalid native contract record size")
	}
	var record struct {
		Version  int    `json:"version"`
		Bytecode string `json:"bytecode"`
		CodeHash string `json:"code_hash"`
		Storage  string `json:"storage"`
	}
	decoder := json.NewDecoder(bytes.NewReader(value))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&record); err != nil {
		return fmt.Errorf("invalid native contract record: %w", err)
	}
	if err := decoder.Decode(new(any)); !errors.Is(err, io.EOF) {
		return errors.New("trailing native contract record data")
	}
	if record.Version != 2 || !canonicalHex(strings.TrimPrefix(record.CodeHash, "0x"), 64) ||
		!strings.HasPrefix(record.CodeHash, "0x") || !strings.HasPrefix(record.Bytecode, "0x") ||
		!strings.HasPrefix(record.Storage, "0x") {
		return errors.New("unsupported native contract record")
	}
	code, err := hex.DecodeString(record.Bytecode[2:])
	if err != nil {
		return errors.New("invalid native bytecode hex")
	}
	if strings.ToLower(record.Bytecode) != record.Bytecode ||
		strings.ToLower(record.Storage) != record.Storage ||
		strings.ToLower(record.CodeHash) != record.CodeHash ||
		strings.ToLower(crypto.Keccak256Hash(code).Hex()) != record.CodeHash {
		return errors.New("native contract code hash mismatch")
	}
	storageBytes, err := hex.DecodeString(record.Storage[2:])
	if err != nil {
		return errors.New("invalid native storage hex")
	}
	var scalar struct {
		Version int                  `json:"version"`
		Words   map[string][32]uint8 `json:"words"`
		Strings map[string]string    `json:"strings"`
		Maps    []json.RawMessage    `json:"maps"`
	}
	if err := json.Unmarshal(storageBytes, &scalar); err != nil || scalar.Version != 1 ||
		scalar.Words == nil || scalar.Strings == nil || scalar.Maps == nil || len(scalar.Maps) != 0 {
		return errors.New("native contract record contains invalid scalar storage")
	}
	return nil
}

// ExportNativeGenesis serializes the *separate* native KV store in its
// deterministic key order. No EVM store contents are included.
func ExportNativeGenesis(store storetypes.KVStore) ([]byte, error) {
	iterator := store.Iterator(nil, nil)
	defer iterator.Close()
	genesis := nativeGenesis{Version: 1, Entries: []nativeGenesisEntry{}}
	used := 0
	contracts := make(map[string]bool)
	maps := make([]string, 0)
	for ; iterator.Valid(); iterator.Next() {
		key, value := string(iterator.Key()), iterator.Value()
		if err := validateNativeEntry(key, value); err != nil {
			return nil, err
		}
		used += len(key) + len(value)
		if len(genesis.Entries) >= nativeGenesisMaxEntries || used > nativeGenesisMaxBytes {
			return nil, errors.New("native genesis export exceeds lab limit")
		}
		genesis.Entries = append(genesis.Entries, nativeGenesisEntry{Key: key, Value: "0x" + hex.EncodeToString(value)})
		if strings.HasPrefix(key, "lithovm/v1/contracts/") {
			contracts[strings.TrimPrefix(key, "lithovm/v1/contracts/")] = true
		} else {
			maps = append(maps, strings.Split(key, "/")[3])
		}
	}
	for _, address := range maps {
		if !contracts[address] {
			return nil, errors.New("orphan native map entry")
		}
	}
	return json.Marshal(genesis)
}

// ImportNativeGenesis fully validates a canonical, sorted snapshot before the
// first Set. A caller must invoke this only on a fresh, empty native store.
func ImportNativeGenesis(store storetypes.KVStore, data []byte) error {
	if len(data) == 0 || len(data) > 2*nativeGenesisMaxBytes {
		return errors.New("invalid native genesis size")
	}
	iterator := store.Iterator(nil, nil)
	notEmpty := iterator.Valid()
	iterator.Close()
	if notEmpty {
		return errors.New("native genesis import requires an empty store")
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	var genesis nativeGenesis
	if err := decoder.Decode(&genesis); err != nil {
		return err
	}
	if err := decoder.Decode(new(any)); !errors.Is(err, io.EOF) {
		return errors.New("trailing native genesis data")
	}
	if genesis.Version != 1 || genesis.Entries == nil || len(genesis.Entries) > nativeGenesisMaxEntries {
		return errors.New("unsupported native genesis")
	}
	type decoded struct {
		key   []byte
		value []byte
	}
	entries := make([]decoded, 0, len(genesis.Entries))
	contracts := make(map[string]bool)
	maps := make([]string, 0)
	previous := ""
	used := 0
	for _, entry := range genesis.Entries {
		if entry.Key <= previous || !strings.HasPrefix(entry.Value, "0x") ||
			strings.ToLower(entry.Value) != entry.Value {
			return errors.New("unordered or noncanonical native genesis entry")
		}
		value, err := hex.DecodeString(entry.Value[2:])
		if err != nil {
			return err
		}
		if err := validateNativeEntry(entry.Key, value); err != nil {
			return err
		}
		used += len(entry.Key) + len(value)
		if used > nativeGenesisMaxBytes {
			return errors.New("native genesis import exceeds lab limit")
		}
		entries = append(entries, decoded{key: []byte(entry.Key), value: value})
		if strings.HasPrefix(entry.Key, "lithovm/v1/contracts/") {
			contracts[strings.TrimPrefix(entry.Key, "lithovm/v1/contracts/")] = true
		} else {
			maps = append(maps, strings.Split(entry.Key, "/")[3])
		}
		previous = entry.Key
	}
	for _, address := range maps {
		if !contracts[address] {
			return errors.New("orphan native map entry")
		}
	}
	for _, entry := range entries {
		store.Set(entry.key, entry.value)
	}
	return nil
}
