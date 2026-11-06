//go:build lithovm_chain_lab

package nativechain

import (
	"bytes"
	"encoding/binary"
	"encoding/hex"
	"os"
	"strings"
	"testing"
)

func TestBinaryRecordMatchesRustFixtureAndRejectsAmbiguousFraming(t *testing.T) {
	text, err := os.ReadFile("testdata/native-v17-record.hex")
	if err != nil {
		t.Fatal(err)
	}
	fixture, err := hex.DecodeString(strings.TrimSpace(string(text)))
	if err != nil {
		t.Fatal(err)
	}
	code, hash, storage, err := decodeNativeRecord(fixture)
	if err != nil || len(hash) != 32 || !bytes.HasPrefix(code, []byte("LITHOVM\x11")) || !bytes.Contains(storage, []byte(`"maps":[]`)) {
		t.Fatalf("decode Rust fixture: %v", err)
	}
	for end := 0; end < len(fixture); end++ {
		if _, _, _, err := decodeNativeRecord(fixture[:end]); err == nil {
			t.Fatalf("accepted truncation at %d", end)
		}
	}
	for _, offset := range []int{5, 9 + len(code) + 32} {
		bad := append([]byte(nil), fixture...)
		binary.BigEndian.PutUint32(bad[offset:offset+4], ^uint32(0))
		if _, _, _, err := decodeNativeRecord(bad); err == nil {
			t.Fatal("accepted oversize length")
		}
	}
	bad := append(append([]byte(nil), fixture...), 0)
	if _, _, _, err := decodeNativeRecord(bad); err == nil {
		t.Fatal("accepted trailing bytes")
	}
	bad = append([]byte(nil), fixture...)
	bad[16] = 16
	if _, _, _, err := decodeNativeRecord(bad); err == nil {
		t.Fatal("accepted v16 binary record")
	}
}
