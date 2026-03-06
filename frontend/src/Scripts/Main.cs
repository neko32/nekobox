using System;
using System.Collections.Generic;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Threading.Tasks;
using Godot;

namespace Nekobox;

/// <summary>
/// nekobox のルートシーン。
/// 起動チェック・設定ロード・会話フローを管理する。
/// </summary>
public partial class Main : Node
{
    // ──── ノード参照 ──────────────────────────────────────────
    private TextureRect     _backgroundRect      = null!;
    private Label           _sessionLabel        = null!;
    private Label           _clockLabel          = null!;
    private ChatWindow      _chatWindow          = null!;
    private InputPopup      _inputPopup          = null!;
    private Node2D          _characterContainer  = null!;

    // ──── 状態 ───────────────────────────────────────────────
    private int                  _lastMinute   = -1;
    private AppConfig            _config       = null!;
    private BackendApiClient     _api          = null!;
    private string               _cfgPath      = string.Empty;
    private string?              _lastResponseId;
    private GDCubismUserModelCS? _cubismModel;
    private Dictionary<string, EmotionEntry>? _emotionMap;

    // emotion_model_map.json の 1 エントリ
    private sealed record EmotionEntry(
        [property: JsonPropertyName("emotion")] string ExpressionId,
        [property: JsonPropertyName("motion")]  string MotionGroup
    );

    // ──── Godot ライフサイクル ────────────────────────────────

    public override void _Ready()
    {
        // ノード参照を取得
        _backgroundRect     = GetNode<TextureRect>("BackgroundLayer/BackgroundRect");
        _sessionLabel       = GetNode<Label>("UILayer/MainControl/SessionLabel");
        _clockLabel         = GetNode<Label>("UILayer/MainControl/ClockLabel");
        _chatWindow         = GetNode<ChatWindow>("UILayer/MainControl/ChatWindow");
        _inputPopup         = GetNode<InputPopup>("UILayer/InputPopup");
        _characterContainer = GetNode<Node2D>("CharacterLayer/CharacterContainer");

        UpdateClock();

        if (!ValidateEnvironment()) return;
        if (!LoadAppConfig())       return;

        LoadEmotionModelMap();
        InitUi();

        // 起動シーケンスを非同期で開始（Godot の SynchronizationContext により await 後も Main Thread）
        _ = StartupAsync();
    }

    public override void _Process(double delta)
    {
        var now = DateTime.Now;
        if (now.Minute == _lastMinute) return;
        _lastMinute = now.Minute;
        UpdateClock();
    }

    private void UpdateClock()
    {
        var now = DateTime.Now;
        string[] dow = ["日", "月", "火", "水", "木", "金", "土"];
        _clockLabel.Text = now.ToString("h:mm tt\nyyyy年M月d日") + $"({dow[(int)now.DayOfWeek]})";
    }

    // ──── 起動チェック ───────────────────────────────────────

    private bool ValidateEnvironment()
    {
        string[] required = ["NEKOBOX_CFG_PATH", "NEKOBOX_DB_PATH",
                             "NEKOBOX_LMSTUDIO_HOST", "NEKOBOX_LMSTUDIO_PORT"];
        foreach (var key in required)
        {
            if (!string.IsNullOrEmpty(System.Environment.GetEnvironmentVariable(key))) continue;

            OS.Alert($"必須環境変数が設定されていません: {key}", "nekobox 起動エラー");
            GetTree().Quit(1);
            return false;
        }

        _cfgPath = System.Environment.GetEnvironmentVariable("NEKOBOX_CFG_PATH")!;
        return true;
    }

    private bool LoadAppConfig()
    {
        try
        {
            _config = AppConfig.Load(_cfgPath);
        }
        catch (Exception e)
        {
            OS.Alert($"app.config のロードに失敗しました:\n{e.Message}", "nekobox 起動エラー");
            GetTree().Quit(1);
            return false;
        }

        // キャラクタープロンプトファイルの存在確認
        var settingsFile = _config.GetCharacterPromptFile();
        if (!System.IO.File.Exists(settingsFile))
        {
            OS.Alert($"キャラクタープロンプトファイルが見つかりません:\n{settingsFile}", "nekobox 起動エラー");
            GetTree().Quit(1);
            return false;
        }

        // バックエンド API クライアント初期化（バックエンドは 127.0.0.1:8080 で待ち受け）
        _api = new BackendApiClient("http://127.0.0.1:8080");

        return true;
    }

    // ──── UI 初期化 ───────────────────────────────────────────

    private void InitUi()
    {
        // セッションラベル
        UpdateSessionLabel();

        // 背景画像をロードしてブラーシェーダを適用
        LoadBackground();

        // Live2D キャラクターをロード
        LoadCharacter();

        // InputPopup のコールバック登録
        _inputPopup.OnMessageSubmitted = OnUserMessageSubmitted;
        _inputPopup.Show();

        // 次フレームで右上に配置（Show の initial_position 処理と競合しないよう Deferred）
        CallDeferred(nameof(PositionInputPopup));
    }

    private void PositionInputPopup()
    {
        // embed_subwindows=true（Godot デフォルト）のため Position はビューポートローカル座標
        const int marginX = 16;
        const int marginY = 40;
        const int popupW  = 520;
        var       vpSize  = DisplayServer.WindowGetSize();   // ビューポートサイズ (1200, 800)
        var       pos     = new Vector2I(vpSize.X - popupW - marginX, marginY);
        GD.Print($"[InputPopup] vpSize={vpSize}  → pos={pos}");
        _inputPopup.Position = pos;
    }

    private void UpdateSessionLabel()
    {
        _sessionLabel.Text = _config.IsFirstSession
            ? "新規セッション"
            : _config.SessionAlias ?? $"Session: {_config.CurrentSession[..Math.Min(8, _config.CurrentSession.Length)]}...";
    }

    private void LoadBackground()
    {
        if (string.IsNullOrEmpty(_config.BackgroundImage) ||
            !System.IO.File.Exists(_config.BackgroundImage))
        {
            GD.PrintErr($"背景画像が見つかりません: {_config.BackgroundImage}");
            return;
        }

        // Godot VFS は絶対 OS パスが不安定なため、.NET で読み込んでバッファ渡し
        // 拡張子ではなく magic bytes でフォーマットを判定（拡張子と中身が一致しない場合を考慮）
        var bytes = System.IO.File.ReadAllBytes(_config.BackgroundImage);
        var img   = new Image();
        var err   = DetectAndLoadImage(img, bytes);

        if (err != Error.Ok)
        {
            GD.PrintErr($"背景画像のロードに失敗しました: {_config.BackgroundImage} ({err})");
            return;
        }

        // TextureRect が stretch_mode=0 (SCALE) で自動的にウィンドウサイズに合わせる
        _backgroundRect.Texture = ImageTexture.CreateFromImage(img);
    }

    private static Error DetectAndLoadImage(Image img, byte[] bytes)
    {
        if (bytes.Length >= 8 &&
            bytes[0] == 0x89 && bytes[1] == 0x50 && bytes[2] == 0x4E && bytes[3] == 0x47)
            return img.LoadPngFromBuffer(bytes);   // PNG

        if (bytes.Length >= 2 && bytes[0] == 0xFF && bytes[1] == 0xD8)
            return img.LoadJpgFromBuffer(bytes);   // JPEG

        if (bytes.Length >= 12 &&
            bytes[0] == 0x52 && bytes[1] == 0x49 && bytes[4] == 0x57 && bytes[5] == 0x45)
            return img.LoadWebpFromBuffer(bytes);  // WebP

        return Error.FileUnrecognized;
    }

    // ──── キャラクター ────────────────────────────────────────

    private void LoadCharacter()
    {
        var modelPath = _config.Character.ModelPath;
        if (string.IsNullOrEmpty(modelPath))
        {
            GD.PrintErr("character.model_path が設定されていません");
            return;
        }

        // OS パスを GDCubism が受け付けるスラッシュ区切りに変換
        var model3Path = System.IO.Path.Combine(modelPath, "007mike.model3.json")
            .Replace('\\', '/');

        if (!System.IO.File.Exists(model3Path.Replace('/', System.IO.Path.DirectorySeparatorChar)))
        {
            GD.PrintErr($"model3.json が見つかりません: {model3Path}");
            return;
        }

        try
        {
            _cubismModel = new GDCubismUserModelCS();
            _cubismModel.PlaybackProcessMode = GDCubismUserModelCS.MotionProcessCallbackEnum.Idle;
            _cubismModel.Assets = model3Path;

            var node = _cubismModel.GetInternalObject();
            _characterContainer.AddChild(node);

            // canvas_info のキャンバスサイズから画面高さ 70% に収まるスケールを計算
            var canvasInfo = _cubismModel.GetCanvasInfo();
            GD.Print($"Canvas: {canvasInfo}");

            var scale = 0.3f; // フォールバック
            if (canvasInfo.TryGetValue("size_in_pixels", out var sizeVar))
            {
                var canvasSize = sizeVar.As<Vector2>();
                var winH       = DisplayServer.WindowGetSize().Y;
                scale = winH * 0.7f / canvasSize.Y;
                GD.Print($"Character scale: {scale} (canvas={canvasSize})");
            }
            node.Scale = new Vector2(scale, scale);

            // 利用可能なモーショングループを表示（デバッグ用）
            var motions = _cubismModel.GetMotions();
            GD.Print($"[Motions] {motions.Count} groups:");
            foreach (var kv in motions)
                GD.Print($"  group=\"{kv.Key}\"  count={kv.Value}");

            // 利用可能な expression を表示（デバッグ用）
            var expressions = _cubismModel.GetExpressions();
            GD.Print($"[Expressions] {expressions.Count} entries:");
            foreach (var ex in expressions)
                GD.Print($"  \"{ex}\"");

            // 感情モーション終了時に Idle へ戻るシグナルを登録
            _cubismModel.MotionFinished += OnMotionFinished;

            // アイドルモーションをループ再生（グループ "Idle" の index 0）
            _cubismModel.StartMotionLoop("Idle", 0, GDCubismUserModelCS.PriorityEnum.PriorityIdle, true, true);
        }
        catch (Exception e)
        {
            GD.PrintErr($"Live2D キャラクターのロードに失敗しました: {e.Message}");
            _cubismModel = null;
        }
    }

    private void LoadEmotionModelMap()
    {
        var mapPath = System.IO.Path.Combine(_cfgPath, "emotion_model_map.json");
        if (!System.IO.File.Exists(mapPath))
        {
            GD.PrintErr($"emotion_model_map.json が見つかりません: {mapPath}");
            return;
        }

        try
        {
            var raw  = System.IO.File.ReadAllText(mapPath);
            var opts = new JsonSerializerOptions { PropertyNameCaseInsensitive = true };
            var root = JsonSerializer.Deserialize<Dictionary<string, Dictionary<string, EmotionEntry>>>(raw, opts);

            if (root is not null && root.TryGetValue(_config.Character.Name, out var charMap))
            {
                _emotionMap = charMap;
                GD.Print($"emotion_model_map: {charMap.Count} エントリ読み込み ({_config.Character.Name})");
            }
            else
            {
                GD.PrintErr($"emotion_model_map.json にキャラクター '{_config.Character.Name}' のエントリがありません");
            }
        }
        catch (Exception e)
        {
            GD.PrintErr($"emotion_model_map.json の読み込みに失敗しました: {e.Message}");
        }
    }

    private void OnMotionFinished()
    {
        // 感情モーション（1回再生）が終わったら Idle に戻る（表情はそのまま保持）
        _cubismModel?.StartMotionLoop("Idle", 0,
            GDCubismUserModelCS.PriorityEnum.PriorityIdle, true, true);
    }

    private void ApplyEmotion(string emotion)
    {
        if (_cubismModel is null) return;

        // emotion_model_map から expression と motion を取得
        var entry = (_emotionMap is not null && _emotionMap.TryGetValue(emotion, out var e))
            ? e
            : new EmotionEntry("normal", "taiki"); // フォールバック

        // StartExpression でクロスフェード遷移させる（"normal" も含めて常に呼ぶ）。
        // StopExpression はパラメータを凍結させることがあるため使わない。
        // normal.exp3.json の Overwrite=0 が能動的にパラメータを 0 に戻す。
        _cubismModel.StartExpression(entry.ExpressionId);

        // PriorityForce で呼ぶことで再生中の emotion モーションを必ず割り込む
        // 終了後は MotionFinished → Idle に戻る
        _cubismModel.StartMotion(entry.MotionGroup, 0,
            GDCubismUserModelCS.PriorityEnum.PriorityForce);
    }

    // ──── 会話フロー ─────────────────────────────────────────

    private async Task StartupAsync()
    {
        // 既存セッションなら履歴を先に復元してからあいさつ
        if (!_config.IsFirstSession)
            await LoadSessionHistoryAsync();

        await StartInitialConversationAsync();
    }

    private async Task LoadSessionHistoryAsync()
    {
        try
        {
            var history = await _api.GetSessionHistoryAsync(_config.CurrentSession);
            var entries = history.Entries;

            // ログは「ユーザー行 → キャラクター行」のペアで保存されている
            for (int i = 0; i + 1 < entries.Count; i += 2)
            {
                var user      = entries[i];
                var character = entries[i + 1];
                _chatWindow.AddMessage(
                    user.UserName, user.Msg,
                    character.MsgSenderName, character.Msg, "普通");
            }
        }
        catch (Exception e)
        {
            _chatWindow.AddError("HISTORY_ERROR", e.Message);
        }
    }

    private async Task StartInitialConversationAsync()
    {
        var initialMsg = _config.IsFirstSession
            ? $"初めまして、私の名前は{_config.UserName}です。自己紹介よろしくね"
            : $"{_config.UserName}だよ、また会いに来たよ";

        await SendMessageAsync(initialMsg);
    }

    private void OnUserMessageSubmitted(string text)
    {
        if (string.IsNullOrWhiteSpace(text)) return;

        // メタコマンド: /new [セッション名] → 新しいセッションを開始
        const string newCmd = "/new";
        if (text.Equals(newCmd, StringComparison.Ordinal) ||
            text.StartsWith(newCmd + " ", StringComparison.Ordinal))
        {
            var alias = text.Length > newCmd.Length ? text[(newCmd.Length + 1)..].Trim() : string.Empty;
            _ = StartNewSessionAsync(alias);
            return;
        }

        // メタコマンド: /emotion <感情名> → バックエンド不要で表情・モーションを直接テスト
        const string emotionCmd = "/emotion ";
        if (text.StartsWith(emotionCmd, StringComparison.Ordinal))
        {
            TestEmotion(text[emotionCmd.Length..].Trim());
            return;
        }

        _ = SendMessageAsync(text);
    }

    private void TestEmotion(string emotion)
    {
        var entry = (_emotionMap is not null && _emotionMap.TryGetValue(emotion, out var e))
            ? e
            : new EmotionEntry("normal", "taiki");

        GD.Print($"[TestEmotion] emotion={emotion} → expression={entry.ExpressionId}, motion group=\"{entry.MotionGroup}\" index=0");

        _chatWindow.AddMessage(
            "[テスト]", $"/emotion {emotion}",
            "[システム]", $"expression={entry.ExpressionId}  motion={entry.MotionGroup}", emotion);

        ApplyEmotion(emotion);
    }

    private async Task StartNewSessionAsync(string alias)
    {
        var newId = Guid.NewGuid().ToString();
        _config.CurrentSession = newId;
        _config.SessionAlias   = string.IsNullOrEmpty(alias) ? newId : alias;
        _config.Save(_cfgPath);

        _lastResponseId = null;
        _chatWindow.Clear();
        UpdateSessionLabel();

        await StartInitialConversationAsync();
    }

    private async Task SendMessageAsync(string userMessage)
    {
        try
        {
            var req = new MsgRequest(
                CharacterName: _config.Character.Name,
                Version:       _config.Character.Version,
                ResponseId:    _lastResponseId,
                ImageUrl:      null,
                UserName:      _config.UserName,
                SessionId:     _config.CurrentSession,
                SessionAlias:  _config.SessionAlias,
                Message:       userMessage
            );

            var res = await _api.SendMessageAsync(req);
            _lastResponseId = res.ResponseId;

            // 初回会話後に current_session を UUID で確定して保存
            if (_config.IsFirstSession)
            {
                _config.CurrentSession = Guid.NewGuid().ToString();
                _config.Save(_cfgPath);
                UpdateSessionLabel();
            }

            _chatWindow.AddMessage(
                _config.UserName, userMessage,
                _config.Character.Name, res.Message, res.Emotion);
            ApplyEmotion(res.Emotion);
        }
        catch (ApiException e)
        {
            _chatWindow.AddError(e.ErrorId, e.Message);
        }
        catch (Exception e)
        {
            _chatWindow.AddError("CLIENT_ERROR", e.Message);
        }
    }
}
