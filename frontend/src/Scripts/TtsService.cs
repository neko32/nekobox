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
}

// ──────────────────────────────────────────────────────────────
//  Godot 組み込み OS-TTS 実装
// ──────────────────────────────────────────────────────────────

public sealed class GodotTtsService : ITtsService
{
    private readonly TtsConfig _config;
    private string? _voiceId;

    public bool IsEnabled => _config.Enabled;

    public GodotTtsService(TtsConfig config)
    {
        _config = config;
        _voiceId = ResolveJapaneseVoice();

        if (_voiceId is not null)
            GD.Print($"[TTS] 日本語音声を検出: {_voiceId}");
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

    // ──── 内部ユーティリティ ─────────────────────────────────

    private static string? ResolveJapaneseVoice()
    {
        // ja → ja-JP の順で検索
        foreach (var lang in new[] { "ja", "ja-JP" })
        {
            var voices = DisplayServer.TtsGetVoicesForLanguage(lang);
            if (voices.Length > 0)
                return voices[0];
        }
        return null;
    }
}
