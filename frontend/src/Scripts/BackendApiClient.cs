using System;
using System.Net.Http;
using System.Net.Http.Json;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Threading;
using System.Threading.Tasks;

namespace Nekobox;

// ──────────────────────────────────────────────────────────────
//  API データモデル
// ──────────────────────────────────────────────────────────────

public sealed record MsgRequest(
    [property: JsonPropertyName("character_name")] string CharacterName,
    [property: JsonPropertyName("version")]        string Version,
    [property: JsonPropertyName("response_id")]    string? ResponseId,
    [property: JsonPropertyName("image_url")]      string? ImageUrl,
    [property: JsonPropertyName("user_name")]      string UserName,
    [property: JsonPropertyName("session_id")]     string SessionId,
    [property: JsonPropertyName("session_alias")]  string? SessionAlias,
    [property: JsonPropertyName("message")]        string Message
);

public sealed record MsgResponse(
    [property: JsonPropertyName("character_name")] string CharacterName,
    [property: JsonPropertyName("version")]        string Version,
    [property: JsonPropertyName("response_id")]    string? ResponseId,
    [property: JsonPropertyName("image_url")]      string? ImageUrl,
    [property: JsonPropertyName("user_name")]      string UserName,
    [property: JsonPropertyName("session_id")]     string SessionId,
    [property: JsonPropertyName("message")]        string Message,
    [property: JsonPropertyName("emotion")]        string Emotion
);

public sealed record ApiErrorResponse(
    [property: JsonPropertyName("id")]      string Id,
    [property: JsonPropertyName("message")] string Message
);

public sealed record SessionLogEntry(
    [property: JsonPropertyName("msg_sender_name")] string MsgSenderName,
    [property: JsonPropertyName("user_name")]       string UserName,
    [property: JsonPropertyName("msg")]             string Msg,
    [property: JsonPropertyName("timestamp")]       string Timestamp
);

public sealed record SessionHistoryResponse(
    [property: JsonPropertyName("session_id")] string SessionId,
    [property: JsonPropertyName("entries")]    System.Collections.Generic.List<SessionLogEntry> Entries
);

// ──────────────────────────────────────────────────────────────
//  例外
// ──────────────────────────────────────────────────────────────

public sealed class ApiException(string errorId, string message)
    : Exception(message)
{
    public string ErrorId { get; } = errorId;
}

// ──────────────────────────────────────────────────────────────
//  クライアント
// ──────────────────────────────────────────────────────────────

public sealed class BackendApiClient : IDisposable
{
    private readonly HttpClient _http;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
    };

    public BackendApiClient(string backendBaseUrl)
    {
        _http = new HttpClient { BaseAddress = new Uri(backendBaseUrl) };
    }

    /// <summary>
    /// POST /v1/msg にメッセージを送り、キャラクターからのレスポンスを受け取る。
    /// </summary>
    public async Task<MsgResponse> SendMessageAsync(
        MsgRequest request,
        CancellationToken ct = default)
    {
        using var httpResponse = await _http.PostAsJsonAsync("/v1/msg", request, JsonOptions, ct);

        if (!httpResponse.IsSuccessStatusCode)
            throw await ReadApiException(httpResponse, ct);

        return await httpResponse.Content.ReadFromJsonAsync<MsgResponse>(JsonOptions, ct)
            ?? throw new InvalidOperationException("バックエンドから空のレスポンスが返りました");
    }

    /// <summary>
    /// GET /v1/sessions/{sessionId} でセッション履歴を取得する。
    /// </summary>
    public async Task<SessionHistoryResponse> GetSessionHistoryAsync(
        string sessionId,
        CancellationToken ct = default)
    {
        using var httpResponse = await _http.GetAsync($"/v1/sessions/{sessionId}", ct);

        if (!httpResponse.IsSuccessStatusCode)
            throw await ReadApiException(httpResponse, ct);

        return await httpResponse.Content.ReadFromJsonAsync<SessionHistoryResponse>(JsonOptions, ct)
            ?? throw new InvalidOperationException("バックエンドから空のレスポンスが返りました");
    }

    /// <summary>
    /// HTTP エラーレスポンスから ApiException を生成する。
    /// ボディが空や JSON でない場合も安全に処理する。
    /// </summary>
    private static async Task<ApiException> ReadApiException(
        HttpResponseMessage response, CancellationToken ct)
    {
        var statusCode = (int)response.StatusCode;
        try
        {
            var body = await response.Content.ReadAsStringAsync(ct);
            if (!string.IsNullOrWhiteSpace(body))
            {
                var err = JsonSerializer.Deserialize<ApiErrorResponse>(body, JsonOptions);
                if (err is not null)
                    return new ApiException(err.Id, err.Message);
            }
        }
        catch { /* JSON パース失敗はフォールバックへ */ }

        return new ApiException("HTTP_ERROR", $"HTTP {statusCode}");
    }

    public void Dispose() => _http.Dispose();
}
