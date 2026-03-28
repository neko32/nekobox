using Godot;

namespace Nekobox;

// ──────────────────────────────────────────────────────────────
//  TTS サービスのインターフェース
//  将来的に VOICEVOX 等の外部 TTS に差し替え可能にするため抽象化する
// ──────────────────────────────────────────────────────────────

public interface ITtsService
{
    /// <summary>テキストを読み上げる。前の発話は中断する。</summary>
    void Speak(string text);

    /// <summary>発話を停止する。</summary>
    void Stop();

    /// <summary>TTS が有効かどうか。</summary>
    bool IsEnabled { get; }

    /// <summary>実行時に TTS の有効/無効を切り替える。</summary>
    void SetEnabled(bool enabled);

    /// <summary>現在使用中の音声 ID を返す。</summary>
    string? CurrentVoiceId { get; }

    /// <summary>システムで利用可能な全音声の一覧を返す。</summary>
    string[] GetAvailableVoices();
}

// ──────────────────────────────────────────────────────────────
//  Godot 組み込み OS-TTS 実装
// ──────────────────────────────────────────────────────────────

public sealed class GodotTtsService : ITtsService
{
    private readonly TtsConfig _config;
    private string? _voiceId;

    public bool IsEnabled => _config.Enabled;
    public string? CurrentVoiceId => _voiceId;

    public GodotTtsService(TtsConfig config)
    {
        _config = config;
        _voiceId = ResolveVoice(config.Voice);

        if (_voiceId is not null)
            GD.Print($"[TTS] 使用音声: {_voiceId}");
        else
            GD.PrintErr("[TTS] 日本語音声が見つかりません。システムに日本語TTSをインストールしてください。");
    }

    public void Speak(string text)
    {
        if (!_config.Enabled) return;
        if (_voiceId is null)   return;
        if (string.IsNullOrWhiteSpace(text)) return;

        // 前の発話を中断してから新しいテキストを読み上げる
        DisplayServer.TtsStop();

        // 最大文字数を超える場合は切り詰める
        var truncated = text.Length > _config.MaxChars
            ? text[.._config.MaxChars]
            : text;

        DisplayServer.TtsSpeak(truncated, _voiceId,
            volume: (int)(_config.Volume * 100f),
            pitch:  1.0f,
            rate:   _config.Rate);
    }

    public void Stop()
    {
        DisplayServer.TtsStop();
    }

    public void SetEnabled(bool enabled)
    {
        _config.Enabled = enabled;
        if (!enabled)
            Stop();
    }

    public string[] GetAvailableVoices()
    {
        var all = DisplayServer.TtsGetVoices();
        var result = new string[all.Count];
        for (int i = 0; i < all.Count; i++)
            result[i] = $"{all[i]["name"].AsString()}  (id: {all[i]["id"].AsString()})";
        return result;
    }

    // ──── 内部ユーティリティ ─────────────────────────────────

    /// <summary>
    /// voice 設定が指定されていれば部分一致で音声を検索。
    /// 未指定の場合は日本語音声（ja → ja-JP）の先頭を返す。
    /// </summary>
    private static string? ResolveVoice(string? voiceName)
    {
        if (!string.IsNullOrWhiteSpace(voiceName))
        {
            // 全音声の "name" フィールドで部分一致検索（大文字小文字無視）
            var all = DisplayServer.TtsGetVoices();
            foreach (var v in all)
            {
                var name = v["name"].AsString();
                var id   = v["id"].AsString();
                if (name.Contains(voiceName, System.StringComparison.OrdinalIgnoreCase))
                    return id;
            }
            GD.PrintErr($"[TTS] 指定音声 '{voiceName}' が見つかりません。日本語音声にフォールバックします。");
        }

        // フォールバック: ja → ja-JP の順で先頭の日本語音声を返す
        foreach (var lang in new[] { "ja", "ja-JP" })
        {
            var voices = DisplayServer.TtsGetVoicesForLanguage(lang);
            if (voices.Length > 0)
                return voices[0];
        }
        return null;
    }
}
