import { ENGLISH_FIXTURE_TEXT, type FixtureText } from "../readmeFixture";

export type SiteLocale = "en" | "ja" | "zh-Hans" | "zh-Hant" | "ko";
export const SITE_LOCALES: SiteLocale[] = ["en", "ja", "zh-Hans", "zh-Hant", "ko"];

export interface SiteText {
  fixture: FixtureText;
  /** Room names for the search corpus rooms (site/fixtures/search-corpus.json). */
  searchRooms: { ja: string; en: string; mixed: string };
  /** A message rendered with Markdown, code and math for the rich scene. */
  rich: { intro: string; item1: string; item2: string; outro: string };
}

export const SITE_TEXT: Record<SiteLocale, SiteText> = {
  en: {
    fixture: ENGLISH_FIXTURE_TEXT,
    searchRooms: { ja: "Tokyo office", en: "Product team", mixed: "Asia community" },
    rich: {
      intro: "Release checklist:",
      item1: "Update the changelog",
      item2: "Run the search tests",
      outro: "The ranking score is"
    }
  },
  ja: {
    fixture: {
      spaces: { lattice: "格子ラボ", photon: "光子読書会", release: "リリース班" },
      rooms: { general: "雑談", design: "デザイン", papers: "論文", random: "なんでも" },
      people: { aki: "アキ", ren: "レン", sora: "ソラ", you: "あなた（Koushi）" },
      messages: [
        "企画ルームへようこそ。",
        "今日の進め方を簡単にまとめました。",
        "メインの流れは、ひと目で追えるようにしたいですね。",
        "賛成です。落ち着いたレイアウトが助かります。",
        "まずは範囲を絞って進めましょう。",
        "ルーム一覧はとても読みやすいです。",
        "次はフォントを確認します。",
        "最終メモはここで共有します。",
        "いいですね、落ち着いてレビューできそうです。"
      ],
      threadLatest: "境界ケースは私が見ます。",
      accountTabs: { work: "仕事", home: "個人", club: "サークル" }
    },
    searchRooms: { ja: "東京オフィス", en: "Product team", mixed: "アジアコミュニティ" },
    rich: { intro: "リリース前のチェック:", item1: "変更履歴を更新", item2: "検索テストを実行", outro: "スコアの式は" }
  },
  "zh-Hans": {
    fixture: {
      spaces: { lattice: "格子实验室", photon: "光子读书会", release: "发布小组" },
      rooms: { general: "综合", design: "设计", papers: "论文", random: "闲聊" },
      people: { aki: "阿明", ren: "小任", sora: "苏拉", you: "你（Koushi）" },
      messages: [
        "欢迎来到策划房间。",
        "我整理了今天的简要提纲。",
        "主流程应该一眼就能看清。",
        "同意，安静的布局很有帮助。",
        "第一轮先集中在重点上吧。",
        "房间列表读起来很清楚。",
        "我接下来检查一下字体。",
        "最终笔记我会发在这里。",
        "很好，可以安心地评审了。"
      ],
      threadLatest: "边界情况我来检查。",
      accountTabs: { work: "工作", home: "个人", club: "社团" }
    },
    searchRooms: { ja: "东京办公室", en: "Product team", mixed: "亚洲社区" },
    rich: { intro: "发布前检查：", item1: "更新变更日志", item2: "运行搜索测试", outro: "排序得分为" }
  },
  "zh-Hant": {
    fixture: {
      spaces: { lattice: "格子實驗室", photon: "光子讀書會", release: "發佈小組" },
      rooms: { general: "綜合", design: "設計", papers: "論文", random: "閒聊" },
      people: { aki: "阿明", ren: "小任", sora: "蘇拉", you: "你（Koushi）" },
      messages: [
        "歡迎來到企劃聊天室。",
        "我整理了今天的簡要大綱。",
        "主要流程應該一眼就能看清楚。",
        "同意，安靜的版面很有幫助。",
        "第一輪先聚焦在重點上吧。",
        "聊天室清單讀起來很清楚。",
        "我接下來檢查一下字型。",
        "最終筆記我會貼在這裡。",
        "很好，可以安心地審閱了。"
      ],
      threadLatest: "邊界情況我來檢查。",
      accountTabs: { work: "工作", home: "個人", club: "社團" }
    },
    searchRooms: { ja: "東京辦公室", en: "Product team", mixed: "亞洲社群" },
    rich: { intro: "發佈前檢查：", item1: "更新變更紀錄", item2: "執行搜尋測試", outro: "排序分數為" }
  },
  ko: {
    fixture: {
      spaces: { lattice: "격자 연구실", photon: "광자 독서회", release: "릴리스 팀" },
      rooms: { general: "일반", design: "디자인", papers: "논문", random: "자유" },
      people: { aki: "아키", ren: "렌", sora: "소라", you: "나 (Koushi)" },
      messages: [
        "기획 방에 오신 것을 환영합니다.",
        "오늘 진행할 내용을 간단히 정리했어요.",
        "주요 흐름은 한눈에 볼 수 있어야 해요.",
        "동의해요. 차분한 레이아웃이 도움이 돼요.",
        "첫 단계는 핵심에 집중합시다.",
        "방 목록이 아주 잘 읽혀요.",
        "다음으로 글꼴을 확인할게요.",
        "최종 메모는 여기에 공유할게요.",
        "좋아요, 차분하게 검토할 수 있겠어요."
      ],
      threadLatest: "예외 상황은 제가 볼게요.",
      accountTabs: { work: "업무", home: "개인", club: "동아리" }
    },
    searchRooms: { ja: "도쿄 사무실", en: "Product team", mixed: "아시아 커뮤니티" },
    rich: { intro: "릴리스 전 확인:", item1: "변경 기록 업데이트", item2: "검색 테스트 실행", outro: "순위 점수는" }
  }
};
