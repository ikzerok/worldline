use crate::LanguageVersion;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct LanguageCapability {
    pub version: LanguageVersion,
    pub title: &'static str,
    pub description: &'static str,
    /// 选择该版本时 core 一并追加的旧客户端兼容保护。
    pub required_features: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct FeatureCapability {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub minimum_language: LanguageVersion,
    pub dependencies: &'static [&'static str],
}

const ENTITY_RELATION: &[&str] = &["content.entities.v1", "content.relations.v1"];
const AUTHORING_112: &[&str] = &[
    "content.entities.v1",
    "content.relations.v1",
    "content.choice_presentation.v1",
];

/// 既有语言版本与实际能力；不与产品版本绑定，不改变默认版本。
pub fn language_capabilities() -> &'static [LanguageCapability] {
    &[
        LanguageCapability {
            version: LanguageVersion::V1_9,
            title: "1.9 · 基础世界与分支叙事（默认）",
            description: "人物、事件、场景、选择、条件、变量、标签与独立状态；时期、同直接时段内先后及静态资料。",
            required_features: &[],
        },
        LanguageCapability {
            version: LanguageVersion::V1_10,
            title: "1.10 · 通用实体与语义关系",
            description: "增加地点、组织、物品等 entity 资料及独立 relation_type/relation_def；静态资料不自动产生运行效果。",
            required_features: ENTITY_RELATION,
        },
        LanguageCapability {
            version: LanguageVersion::V1_11,
            title: "1.11 · 规则、片段、集合与台词",
            description: "继承 1.10，增加纯 rule、可返回 fragment/call、local、标签集合、动态状态动作及 say 人物台词；旧关键字正文可能重新解释。",
            required_features: ENTITY_RELATION,
        },
        LanguageCapability {
            version: LanguageVersion::V1_12,
            title: "1.12 · 资料约束与可见禁用选择",
            description: "继承 1.11，增加 schema/field/bind 持续资料约束及 choice enable/disabled；显示条件与可选条件分开。",
            required_features: AUTHORING_112,
        },
        LanguageCapability {
            version: LanguageVersion::V1_13,
            title: "1.13 · 同根跨时段偏序与人物引用",
            description: "继承 1.12，同一顶层时期下可跨后代时段声明 follows；不是日历运算或自动调度。静态人物强引用须另选双能力。",
            required_features: AUTHORING_112,
        },
    ]
}

/// 只列此事务可追加的既有内容能力；其他展示能力随原清单保留。
pub fn feature_capabilities() -> &'static [FeatureCapability] {
    &[
        FeatureCapability {
            id: "content.entities.v1",
            title: "实体资料",
            description: "地点、组织、物品等通用实体；版本 1.10 起的选择会一并声明。",
            minimum_language: LanguageVersion::V1_10,
            dependencies: &[],
        },
        FeatureCapability {
            id: "content.relations.v1",
            title: "语义关系",
            description: "独立关系类型、方向、端点与关系说明；不等于执行跳转。",
            minimum_language: LanguageVersion::V1_10,
            dependencies: &[],
        },
        FeatureCapability {
            id: "content.choice_presentation.v1",
            title: "可见禁用选择",
            description: "choice enable/disabled 显示不能选的选项与作者说明；不泄露调试状态。",
            minimum_language: LanguageVersion::V1_12,
            dependencies: &[],
        },
        FeatureCapability {
            id: "content.object_refs.v1",
            title: "静态对象强引用",
            description: "属性 ref(\"entity\", \"id\") 或 ref(\"relation\", \"id\")；参与类型、缺失目标和改名检查。",
            minimum_language: LanguageVersion::V1_10,
            dependencies: &[],
        },
        FeatureCapability {
            id: "content.character_refs.v1",
            title: "静态人物强引用",
            description: "人物属性 ref、schema ref character 与人物模板字段；不是动态人物参数或动态 speaker。",
            minimum_language: LanguageVersion::V1_13,
            dependencies: &["content.object_refs.v1"],
        },
        FeatureCapability {
            id: "content.localization.v1",
            title: "本地化文本身份",
            description: "#wl-localization:ID 是源码身份元数据；不自动翻译，也不切换当前演练输出。",
            minimum_language: LanguageVersion::V1_9,
            dependencies: &[],
        },
    ]
}

pub(super) fn rank(version: LanguageVersion) -> usize {
    LanguageVersion::SUPPORTED
        .iter()
        .position(|item| *item == version)
        .expect("LanguageVersion 必须来自支持集合")
}
