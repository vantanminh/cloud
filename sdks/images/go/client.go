package knotreeimages

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
)

type Error struct {
	Status  int
	Code    string
	Message string
	Fields  map[string]string
}

func (e *Error) Error() string {
	return e.Message
}

type Client struct {
	ClientID     string
	ClientSecret string
	BaseURL      string
	HTTP         *http.Client
}

type Store struct {
	ID              string `json:"id"`
	Name            string `json:"name"`
	ResourceType    string `json:"resourceType"`
	CompressionMode string `json:"compressionMode"`
	MaxWidth        *int   `json:"maxWidth"`
	MaxHeight       *int   `json:"maxHeight"`
	Quality         *int   `json:"quality"`
	ObjectCount     int64  `json:"objectCount"`
	ByteSize        int64  `json:"byteSize"`
	PublicBaseURL   string `json:"publicBaseUrl"`
}

type Object struct {
	ID          string `json:"id"`
	Folder      string `json:"folder"`
	FileName    string `json:"fileName"`
	ContentType string `json:"contentType"`
	ByteSize    int64  `json:"byteSize"`
	Width       int    `json:"width"`
	Height      int    `json:"height"`
	CreatedAt   string `json:"createdAt"`
}

type ObjectList struct {
	Objects []Object `json:"objects"`
	Folders []string `json:"folders"`
}

type SignedURL struct {
	URL          string  `json:"url"`
	Visibility   string  `json:"visibility"`
	ExpiresAt    *string `json:"expiresAt"`
	CacheSeconds uint32  `json:"cacheSeconds"`
	ContentType  string  `json:"contentType"`
}

type DeletedFolder struct {
	Deleted uint64 `json:"deleted"`
}

type Upload struct {
	Bytes       []byte
	ContentType string
	Folder      string
	FileName    string
}

type ListOptions struct {
	Folder    string
	Recursive bool
}

type SignOptions struct {
	Visibility       string
	ExpiresInSeconds *int64
	Width            *uint32
	Height           *uint32
	Quality          *uint32
}

func New(clientID, clientSecret, baseURL string) (*Client, error) {
	if clientID == "" || clientSecret == "" {
		return nil, &Error{Code: "IMAGE_CONFIG", Message: "client id and client secret are required"}
	}
	origin, err := normalizeBaseURL(baseURL)
	if err != nil {
		return nil, err
	}
	return &Client{
		ClientID:     clientID,
		ClientSecret: clientSecret,
		BaseURL:      origin,
		HTTP:         http.DefaultClient,
	}, nil
}

func (c *Client) GetStore(ctx context.Context) (*Store, error) {
	var store Store
	err := c.do(ctx, http.MethodGet, "/images/store", nil, nil, nil, &store)
	return &store, err
}

func (c *Client) Upload(ctx context.Context, image Upload) (*Object, error) {
	if image.FileName == "" {
		return nil, &Error{Code: "IMAGE_CONFIG", Message: "file name is required"}
	}
	headers := map[string]string{
		"Content-Type":        fallback(image.ContentType, "application/octet-stream"),
		"X-Knotree-Folder":    image.Folder,
		"X-Knotree-File-Name": image.FileName,
	}
	var object Object
	err := c.do(ctx, http.MethodPost, "/images/objects", nil, headers, image.Bytes, &object)
	return &object, err
}

func (c *Client) List(ctx context.Context, options ListOptions) (*ObjectList, error) {
	query := url.Values{}
	if options.Folder != "" {
		query.Set("folder", options.Folder)
	}
	if options.Recursive {
		query.Set("recursive", "true")
	} else {
		query.Set("recursive", "false")
	}
	var list ObjectList
	err := c.do(ctx, http.MethodGet, "/images/objects", query, nil, nil, &list)
	return &list, err
}

func (c *Client) Delete(ctx context.Context, imageID string) error {
	return c.do(ctx, http.MethodDelete, "/images/objects/"+url.PathEscape(imageID), nil, nil, nil, nil)
}

func (c *Client) DeleteFolder(ctx context.Context, folder string) (*DeletedFolder, error) {
	query := url.Values{}
	query.Set("folder", folder)
	var deleted DeletedFolder
	err := c.do(ctx, http.MethodDelete, "/images/folders", query, nil, nil, &deleted)
	return &deleted, err
}

func (c *Client) SignURL(ctx context.Context, imageID string, options SignOptions) (*SignedURL, error) {
	visibility := options.Visibility
	if visibility == "" {
		visibility = "public"
	}
	body := map[string]any{"visibility": visibility}
	if options.ExpiresInSeconds != nil {
		body["expiresInSeconds"] = *options.ExpiresInSeconds
	}
	if options.Width != nil {
		body["width"] = *options.Width
	}
	if options.Height != nil {
		body["height"] = *options.Height
	}
	if options.Quality != nil {
		body["quality"] = *options.Quality
	}
	encoded, err := json.Marshal(body)
	if err != nil {
		return nil, err
	}
	headers := map[string]string{"Content-Type": "application/json"}
	var signed SignedURL
	err = c.do(ctx, http.MethodPost, "/images/objects/"+url.PathEscape(imageID)+"/sign", nil, headers, encoded, &signed)
	return &signed, err
}

func (c *Client) do(ctx context.Context, method, path string, query url.Values, headers map[string]string, body []byte, dest any) error {
	endpoint, err := url.Parse(c.BaseURL + "/api/v1" + path)
	if err != nil {
		return err
	}
	if len(query) > 0 {
		endpoint.RawQuery = query.Encode()
	}
	var reader io.Reader
	if body != nil {
		reader = bytes.NewReader(body)
	}
	request, err := http.NewRequestWithContext(ctx, method, endpoint.String(), reader)
	if err != nil {
		return err
	}
	request.Header.Set("X-Knotree-Client-Id", c.ClientID)
	request.Header.Set("X-Knotree-Client-Secret", c.ClientSecret)
	for key, value := range headers {
		request.Header.Set(key, value)
	}
	httpClient := c.HTTP
	if httpClient == nil {
		httpClient = http.DefaultClient
	}
	response, err := httpClient.Do(request)
	if err != nil {
		return err
	}
	defer response.Body.Close()
	if response.StatusCode == http.StatusNoContent {
		return nil
	}
	payload, err := io.ReadAll(response.Body)
	if err != nil {
		return err
	}
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return apiError(response.StatusCode, payload)
	}
	if dest == nil || len(payload) == 0 {
		return nil
	}
	return json.Unmarshal(payload, dest)
}

func normalizeBaseURL(baseURL string) (string, error) {
	trimmed := strings.TrimRight(strings.TrimSpace(baseURL), "/")
	if trimmed == "" {
		return "", &Error{Code: "IMAGE_CONFIG", Message: "base URL is required"}
	}
	trimmed = strings.TrimSuffix(trimmed, "/api/v1")
	return strings.TrimRight(trimmed, "/"), nil
}

func fallback(value, fallback string) string {
	if value == "" {
		return fallback
	}
	return value
}

func apiError(status int, payload []byte) error {
	var envelope struct {
		Error struct {
			Code    string            `json:"code"`
			Message string            `json:"message"`
			Fields  map[string]string `json:"fields"`
		} `json:"error"`
	}
	_ = json.Unmarshal(payload, &envelope)
	if envelope.Error.Code == "" {
		envelope.Error.Code = "IMAGE_REQUEST_FAILED"
	}
	if envelope.Error.Message == "" {
		envelope.Error.Message = fmt.Sprintf("image request failed (%d)", status)
	}
	return &Error{
		Status:  status,
		Code:    envelope.Error.Code,
		Message: envelope.Error.Message,
		Fields:  envelope.Error.Fields,
	}
}
