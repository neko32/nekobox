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
    /// 背景画像パス。仕様書サンプルでは "backend_image" と記載ゆれあり → "background_image" を正とする。
    /// </summary>
    [JsonPropertyName("background_image")]
    public string? BackgroundImage { get; set; }

    [JsonPropertyName("character")]
    public CharacterConfig Character { get; set; } = new();

    [JsonPropertyName("model")]
    public ModelConfig Model { get; set; } = new();

    // ──── ヘルパー ────────────────────────────────────────────

    /// <summary>初回セッション（current_session == "na"）か</summary>
    [System.Text.Json.Serialization.JsonIgnore]
    public bool IsFirstSession => CurrentSession == "na";

    /// <summary>キャラクター設定ファイルのフルパスを返す</summary>
    public string GetCharacterSettingsFile() =>
        Path.Combine(Character.SettingsPath, $"{Character.Name}_{Character.Version}.json");

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
        cfg.BackgroundImage        = cfg.BackgroundImage is null ? null : ExpandPath(cfg.BackgroundImage);

        return cfg;
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
    [JsonPropertyName("temperature")]
    public float Temperature { get; set; } = 0.6f;
}
