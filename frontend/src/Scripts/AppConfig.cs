using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.RegularExpressions;

namespace Nekobox;

// ──────────────────────────────────────────────────────────────
//  app.config の JSON モデル
// ──────────────────────────────────────────────────────────────

public sealed class AppConfig
{
    [JsonPropertyName("current_session")]
    public string CurrentSession { get; set; } = "na";

    [JsonPropertyName("session_alias")]
    public string? SessionAlias { get; set; }

    [JsonPropertyName("user_name")]
    public string UserName { get; set; } = string.Empty;

    /// <summary>
    /// background_config.json 内の背景 ID。
    /// </summary>
    [JsonPropertyName("background_id")]
    public string? BackgroundId { get; set; }

    [JsonPropertyName("character")]
    public CharacterConfig Character { get; set; } = new();

    [JsonPropertyName("model")]
    public ModelConfig Model { get; set; } = new();

    [JsonPropertyName("tts")]
    public TtsConfig Tts { get; set; } = new();

    // ──── ヘルパー ────────────────────────────────────────────

    /// <summary>初回セッション（current_session == "na"）か</summary>
    [System.Text.Json.Serialization.JsonIgnore]
    public bool IsFirstSession => CurrentSession == "na";

    /// <summary>キャラクタープロンプトファイルのフルパスを返す (.md)</summary>
    public string GetCharacterPromptFile() =>
        Path.Combine(Character.SettingsPath, $"{Character.Name}_{Character.Version}.md");

    /// <summary>
    /// キャラクタープロンプト (.md) を読み込み、{{name}} をユーザー名に置換して返す。
    /// </summary>
    public string LoadSystemPrompt()
    {
        var path = GetCharacterPromptFile();
        var raw = File.ReadAllText(path);
        return raw.Replace("{{name}}", UserName);
    }

    // ──── 永続化 ─────────────────────────────────────────────

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true,
        WriteIndented = true,
        Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
    };

    public static AppConfig Load(string cfgPath)
    {
        var file = Path.Combine(cfgPath, "app.config");
        if (!File.Exists(file))
            throw new FileNotFoundException($"app.config が見つかりません: {file}");

        var raw = File.ReadAllText(file);
        var cfg = JsonSerializer.Deserialize<AppConfig>(raw, JsonOptions)
            ?? throw new InvalidOperationException("app.config のデシリアライズに失敗しました");

        // パス系フィールドの環境変数展開 & OS パスデリミタ正規化
        cfg.Character.SettingsPath = ExpandPath(cfg.Character.SettingsPath)!;
        cfg.Character.ModelPath    = ExpandPath(cfg.Character.ModelPath);

        return cfg;
    }

    /// <summary>
    /// background_config.json をロードし、BackgroundId に一致するエントリを返す。
    /// BackgroundId が未設定またはマッチするエントリがなければ null を返す。
    /// </summary>
    public BackgroundEntry? LoadBackground(string cfgPath)
    {
        if (string.IsNullOrEmpty(BackgroundId)) return null;

        var file = Path.Combine(cfgPath, "background_config.json");
        if (!File.Exists(file)) return null;

        var raw    = File.ReadAllText(file);
        var parsed = JsonSerializer.Deserialize<BackgroundConfigFile>(raw, JsonOptions);
        if (parsed?.Background is null) return null;

        var entry = parsed.Background.Find(b => b.Id == BackgroundId);
        if (entry is null) return null;

        // image パス内の環境変数を展開
        entry.Image = ExpandPath(entry.Image) ?? entry.Image;
        return entry;
    }

    public void Save(string cfgPath)
    {
        var file = Path.Combine(cfgPath, "app.config");
        File.WriteAllText(file, JsonSerializer.Serialize(this, JsonOptions));
    }

    // ──── 内部ユーティリティ ─────────────────────────────────

    private static readonly Regex EnvVarPattern =
        new(@"\$\{([^}]+)\}", RegexOptions.Compiled);

    private static string? ExpandPath(string? path)
    {
        if (path is null) return null;

        // ${VAR} を環境変数で展開
        var expanded = EnvVarPattern.Replace(path, m =>
            Environment.GetEnvironmentVariable(m.Groups[1].Value) ?? m.Value);

        // OS のパスデリミタに統一
        return RuntimeInformation.IsOSPlatform(OSPlatform.Windows)
            ? expanded.Replace('/', '\\')
            : expanded.Replace('\\', '/');
    }
}

public sealed class CharacterConfig
{
    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("version")]
    public string Version { get; set; } = string.Empty;

    [JsonPropertyName("model_path")]
    public string? ModelPath { get; set; }

    [JsonPropertyName("settings_path")]
    public string SettingsPath { get; set; } = string.Empty;
}

public sealed class ModelConfig
{
    [JsonPropertyName("regular_chat")]
    public ChatModelConfig RegularChat { get; set; } = new();

    [JsonPropertyName("summary_gen")]
    public ChatModelConfig SummaryGen { get; set; } = new();
}

public sealed class ChatModelConfig
{
    [JsonPropertyName("temperature")]
    public float Temperature { get; set; } = 0.6f;
}

// ──────────────────────────────────────────────────────────────
//  TTS 設定
// ──────────────────────────────────────────────────────────────

public sealed class TtsConfig
{
    /// <summary>TTS を有効にするか（デフォルト: false）</summary>
    [JsonPropertyName("enabled")]
    public bool Enabled { get; set; } = false;

    /// <summary>読み上げ速度（0.1 〜 10.0、デフォルト: 1.0）</summary>
    [JsonPropertyName("rate")]
    public float Rate { get; set; } = 1.0f;

    /// <summary>音量（0.0 〜 1.0、デフォルト: 1.0）</summary>
    [JsonPropertyName("volume")]
    public float Volume { get; set; } = 1.0f;

    /// <summary>読み上げる最大文字数（デフォルト: 500）</summary>
    [JsonPropertyName("max_chars")]
    public int MaxChars { get; set; } = 500;

    /// <summary>
    /// 使用する音声名（部分一致）。null または空文字の場合は日本語音声の先頭を自動選択。
    /// /tts voices コマンドで利用可能な音声一覧を確認できる。
    /// </summary>
    [JsonPropertyName("voice")]
    public string? Voice { get; set; }
}

// ──────────────────────────────────────────────────────────────
//  background_config.json のモデル
// ──────────────────────────────────────────────────────────────

public sealed class BackgroundEntry
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    /// <summary>環境変数展開済みの画像パス</summary>
    [JsonPropertyName("image")]
    public string Image { get; set; } = string.Empty;

    [JsonPropertyName("description")]
    public string Description { get; set; } = string.Empty;

    [JsonPropertyName("location_type")]
    public List<string> LocationType { get; set; } = [];
}

internal sealed class BackgroundConfigFile
{
    [JsonPropertyName("background")]
    public List<BackgroundEntry>? Background { get; set; }
}
