这里是来自 [DeepSeek API Docs](https://api-docs.deepseek.com/zh-cn/quick_start/token_usage) 的`added_tokens`的汇总:

| 标记符                                                 | special  | normalized | 用途                                                         |
| ------------------------------------------------------ | -------- | ---------- | ------------------------------------------------------------ |
| `<think></think>`                                      | false    | **true**   | **推理链容器**（Chain-of-Thought）。DeepSeek-R1 等推理模型在生成最终回答前，会在此标签内输出内部思考过程，对外通常折叠显示。 |
| `<｜fim▁hole｜>` / `<｜fim▁begin｜>` / `<｜fim▁end｜>` | false    | **true**   | **Fill-In-the-Middle（代码中间补全）**。`begin` 和 `end` 标记前缀/后缀代码块，`hole` 标记需要模型填充的中间位置。 |
| `<｜User｜>` / `<｜Assistant｜>`                       | false    | **true**   | **角色锚点**。替代传统的 `User:` / `Assistant:` 文本前缀，作为更鲁棒的结构化分隔符，防止角色混淆攻击（prompt injection）。 |
| `<\|EOT\|>`                                              | **true** | **true**   | **End of Turn**。标记当前轮次（turn）的结束，是模型停止生成的信号之一。 |
| `<｜tool▁calls▁begin｜>` / `<｜tool▁calls▁end｜>`      | false    | **true**   | **工具调用列表容器**。包裹本轮所有需要调用的工具。           |
| `<｜tool▁call▁begin｜>` / `<｜tool▁call▁end｜>`        | false    | **true**   | **单个工具调用容器**。内部通常包含 JSON 格式的函数名和参数。 |
| `<｜tool▁outputs▁begin｜>` / `<｜tool▁outputs▁end｜>`  | false    | **true**   | **工具返回结果列表容器**。                                   |
| `<｜tool▁output▁begin｜>` / `<｜tool▁output▁end｜>`    | false    | **true**   | **单个工具返回结果容器**。                                   |
| `<｜tool▁sep｜>`                                       | false    | **true**   | **工具分隔符**。用于分隔同一轮中的多个工具调用或返回结果。   |
| `<｜begin▁of▁sentence｜>` / `<｜end▁of▁sentence｜>`    | **true** | false      | **序列级边界标记**（BOS/EOS）。标记整个输入/输出序列的物理开始和结束。 |
| `<｜▁pad▁｜>`                                          | **true** | false      | **填充标记**（PAD）。batch 推理时用于对齐序列长度，模型不会对其生成注意力。 |

然后这里是实际在deepseek网页端的对话测试

![image-20260429105126264](assets/图1.png)

通过如上的这个张图可以在被网页后端过滤后真正可以使用的标记符只有`<think></think>` `<｜User｜>` `<｜Assistant｜>`, 所以

- 我打算使用 `< | System | >`进行妥协的系统提示词注入;
- ~~将通过指令规则限定模型使用特殊的模式进行工具调用, 使用 `< | Tool | >`表示工具调用结果;~~
- 同时如下图所示, `<think>`在不闭合的情况下可以引导模型进行思考, 这样就可以进行更加强力规则的注入(reminder);

![image-20260429110516352](assets/图2.png)

## 后续实验发现

经过实际测试, 原生标签 `<｜tool▁calls▁begin｜>` 作为主标签时模型严重混淆, 怀疑后端对 `<｜...｜>` 全角格式有特殊处理或过滤。

尝试了折中方案 `<|tool▁calls▁begin|>` / `<|tool▁calls▁end|>` 作为工具调用标签:

- 使用 ASCII `|` 替代全角 `｜`, 既不触发后端过滤, 又保留了类原生标签的结构感
- **效果意外很好**, 模型识别和遵循度明显提升, 幻觉也大幅减少
- 可能原因是 tokenizer 对 `<|...|>` 格式有已有的 token 模式, 模型对这个"结构模板"有更好的遵循倾向

**当前策略: 实验驱动, 增量维护。**

- 主标签: `<|tool▁calls▁begin|>` / `<|tool▁calls▁end|>`
- 回退列表默认为空, 发现模型输出幻觉变体时再逐个追加到 `extra_starts` / `extra_ends`
- `<|tool▁calls▁begin|>` 格式模型几乎不产生幻觉, 省去了大量回退维护成本
