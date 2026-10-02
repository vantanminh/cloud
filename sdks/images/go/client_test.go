package knotreeimages

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestClientSignsAndDeletes(t *testing.T) {
	var calls []http.Request
	var bodies []string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		calls = append(calls, *r)
		bodies = append(bodies, string(body))
		switch {
		case r.Method == http.MethodDelete && r.URL.Path == "/api/v1/images/objects/img-1":
			w.WriteHeader(http.StatusNoContent)
		case r.Method == http.MethodGet && r.URL.Path == "/api/v1/images/store":
			writeJSON(w, map[string]any{
				"id": "store", "name": "Website", "resourceType": "images",
				"compressionMode": "per_url", "maxWidth": 1600, "maxHeight": nil,
				"quality": 80, "objectCount": 0, "byteSize": 0,
				"publicBaseUrl": "https://img.knotree.org",
			})
		case r.Method == http.MethodPost && r.URL.Path == "/api/v1/images/objects":
			writeJSON(w, map[string]any{
				"id": "img-1", "folder": "posts", "fileName": "cover.png",
				"contentType": "image/png", "byteSize": 3, "width": 1, "height": 1,
				"createdAt": "2026-10-02T00:00:00Z",
			})
		case r.Method == http.MethodGet && r.URL.Path == "/api/v1/images/objects":
			writeJSON(w, map[string]any{"objects": []any{}, "folders": []string{"posts"}})
		case r.Method == http.MethodDelete && r.URL.Path == "/api/v1/images/folders":
			writeJSON(w, map[string]any{"deleted": 1})
		case r.Method == http.MethodPost && r.URL.Path == "/api/v1/images/objects/img-1/sign":
			writeJSON(w, map[string]any{
				"url":          "https://img.knotree.org/images/v1/store/img-1?mode=per_url&w=800&q=70&sig=abc",
				"visibility":   "public",
				"expiresAt":    nil,
				"cacheSeconds": 31536000,
				"contentType":  "image/webp",
			})
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	client, err := New("kimg_browser", "ksec_secret", server.URL+"/api/v1")
	if err != nil {
		t.Fatal(err)
	}
	if client.BaseURL != server.URL {
		t.Fatalf("base url = %s", client.BaseURL)
	}
	store, err := client.GetStore(context.Background())
	if err != nil || store.CompressionMode != "per_url" {
		t.Fatalf("store %#v %v", store, err)
	}
	object, err := client.Upload(context.Background(), Upload{
		Bytes: []byte("png"), ContentType: "image/png", Folder: "posts", FileName: "cover.png",
	})
	if err != nil || object.ID != "img-1" {
		t.Fatalf("upload %#v %v", object, err)
	}
	list, err := client.List(context.Background(), ListOptions{Folder: "posts", Recursive: true})
	if err != nil || len(list.Folders) != 1 || list.Folders[0] != "posts" {
		t.Fatalf("list %#v %v", list, err)
	}
	width := uint32(800)
	quality := uint32(70)
	signed, err := client.SignURL(context.Background(), object.ID, SignOptions{
		Visibility: "public", Width: &width, Quality: &quality,
	})
	if err != nil || signed.CacheSeconds != 31536000 {
		t.Fatalf("signed %#v %v", signed, err)
	}
	if err := client.Delete(context.Background(), object.ID); err != nil {
		t.Fatal(err)
	}
	deleted, err := client.DeleteFolder(context.Background(), "posts")
	if err != nil || deleted.Deleted != 1 {
		t.Fatalf("deleted %#v %v", deleted, err)
	}

	if len(calls) != 6 {
		t.Fatalf("calls = %d", len(calls))
	}
	for _, call := range calls {
		if call.Header.Get("X-Knotree-Client-Id") != "kimg_browser" || call.Header.Get("X-Knotree-Client-Secret") != "ksec_secret" {
			t.Fatalf("missing credentials: %#v", call.Header)
		}
	}
	if calls[1].Header.Get("X-Knotree-Folder") != "posts" || calls[1].Header.Get("X-Knotree-File-Name") != "cover.png" {
		t.Fatalf("upload headers: %#v", calls[1].Header)
	}
	var signedBody map[string]any
	if err := json.Unmarshal([]byte(bodies[3]), &signedBody); err != nil {
		t.Fatalf("sign body %s: %v", bodies[3], err)
	}
	if signedBody["visibility"] != "public" || signedBody["width"] != float64(800) || signedBody["quality"] != float64(70) {
		t.Fatalf("sign body %#v", signedBody)
	}
}

func TestRevokedKey(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
		_, _ = w.Write([]byte(`{"error":{"code":"IMAGE_KEY_REVOKED","message":"This image client key has been revoked."}}`))
	}))
	defer server.Close()
	client, err := New("kimg_browser", "ksec_secret", server.URL)
	if err != nil {
		t.Fatal(err)
	}
	_, err = client.SignURL(context.Background(), "img-1", SignOptions{Visibility: "private"})
	api, ok := err.(*Error)
	if !ok || api.Status != 401 || api.Code != "IMAGE_KEY_REVOKED" {
		t.Fatalf("error %#v", err)
	}
}

func TestMissingCredentials(t *testing.T) {
	if _, err := New("", "secret", "http://localhost"); err == nil {
		t.Fatal("expected config error")
	}
}

func writeJSON(w http.ResponseWriter, payload any) {
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(payload)
}
