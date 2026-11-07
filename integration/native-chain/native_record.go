//go:build lithovm_chain_lab

package nativechain

import (
	"bytes"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"strings"
)

// decodeNativeRecord preserves the v2 JSON codec for bytecode through v16.
// Newly compiled v17 contracts use LCTR3 binary framing without hex expansion.
// The caller still validates the code hash and scalar/per-key storage schema.
func decodeNativeRecord(value []byte) (code, hash, storage []byte, err error) {
	invalid := errors.New("invalid native contract record")
	if len(value) == 0 || len(value) > 2*1024*1024 {
		return nil, nil, nil, invalid
	}
	if bytes.HasPrefix(value, []byte("LCTR\x03")) {
		remaining := value[5:]
		take := func(size int) ([]byte, bool) {
			if size > len(remaining) {
				return nil, false
			}
			part := remaining[:size]
			remaining = remaining[size:]
			return part, true
		}
		field := func(limit uint32) ([]byte, bool) {
			length, ok := take(4)
			if !ok {
				return nil, false
			}
			n := binary.BigEndian.Uint32(length)
			if n > limit {
				return nil, false
			}
			return take(int(n))
		}
		var ok bool
		code, ok = field(65536)
		if !ok || len(code) < 8 || !bytes.Equal(code[:8], []byte("LITHOVM\x11")) {
			return nil, nil, nil, invalid
		}
		hash, ok = take(32)
		if !ok {
			return nil, nil, nil, invalid
		}
		storage, ok = field(2 * 1024 * 1024)
		if !ok || len(remaining) != 0 {
			return nil, nil, nil, invalid
		}
		return code, hash, storage, nil
	}
	var record struct {
		Version  int    `json:"version"`
		Bytecode string `json:"bytecode"`
		CodeHash string `json:"code_hash"`
		Storage  string `json:"storage"`
	}
	decoder := json.NewDecoder(bytes.NewReader(value))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&record) != nil {
		return nil, nil, nil, invalid
	}
	if !errors.Is(decoder.Decode(new(any)), io.EOF) || record.Version != 2 {
		return nil, nil, nil, invalid
	}
	fields := make([][]byte, 0, 3)
	for _, s := range []string{record.Bytecode, record.CodeHash, record.Storage} {
		if !strings.HasPrefix(s, "0x") || strings.ToLower(s) != s {
			return nil, nil, nil, invalid
		}
		field, e := hex.DecodeString(s[2:])
		if e != nil {
			return nil, nil, nil, invalid
		}
		fields = append(fields, field)
	}
	code, hash, storage = fields[0], fields[1], fields[2]
	if len(hash) != 32 || (len(code) > 7 && code[7] >= 17) {
		return nil, nil, nil, invalid
	}
	return code, hash, storage, nil
}
