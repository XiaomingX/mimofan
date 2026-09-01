#!/usr/bin/env bash
set -uo pipefail

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 测试配置
API_KEY="${MIMOFAN_TEST_API_KEY:?set MIMOFAN_TEST_API_KEY in CI secret}"
BASE_URL="https://api.xiaomimimo.com/v1"
MODEL="mimo-v2.5"

test_number=0
pass_count=0
fail_count=0
failures=()

log_test() {
  local test_num=$1
  local result=$2
  if [[ "$result" == "pass" ]]; then
    echo -e "  ${GREEN}✓ 测试 $test_num: 通过${NC}"
    ((pass_count++))
  else
    echo -e "  ${RED}✗ 测试 $test_num: 失败${NC}"
    ((fail_count++))
    failures+=("$test_num")
  fi
  ((test_number++))
}

echo ""
echo -e "${BLUE}══════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}     扩展工具验收测试（tools_extended_test.sh）${NC}"
echo -e "${BLUE}══════════════════════════════════════════════════════════════${NC}"
echo ""

# ════════════════════════════════════════════════════════════════
# 工具 1: revert_turn（回滚操作）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: revert_turn（回滚操作）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"请帮我读取当前目录下的文件列表，然后使用 revert_turn 工具回滚操作\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"revert_turn\",
        \"description\": \"回滚最近的工具操作，恢复到之前的状态\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"turn_id\": {
              \"type\": \"string\",
              \"description\": \"要回滚的轮次ID\"
            }
          },
          \"required\": [\"turn_id\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "revert_turn 工具定义" "pass"
else
  log_test "revert_turn 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 2: automation_create（自动化创建）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: automation_create（自动化创建）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"创建一个自动化任务\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"automation_create\",
        \"description\": \"创建一个新的自动化任务\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"name\": {\"type\": \"string\", \"description\": \"任务名称\"},
            \"description\": {\"type\": \"string\", \"description\": \"任务描述\"},
            \"schedule\": {\"type\": \"string\", \"description\": \"Cron 表达式\"}
          },
          \"required\": [\"name\", \"description\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "automation_create 工具定义" "pass"
else
  log_test "automation_create 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 3: automation_list（自动化列表）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: automation_list（自动化列表）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"列出所有自动化任务\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"automation_list\",
        \"description\": \"列出所有自动化任务\",
        \"parameters\": {\"type\": \"object\", \"properties\": {}}
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "automation_list 工具定义" "pass"
else
  log_test "automation_list 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 4: automation_read（自动化读取）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: automation_read（自动化读取）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"读取自动化任务详情\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"automation_read\",
        \"description\": \"读取自动化任务详情\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"task_id\": {\"type\": \"string\", \"description\": \"任务ID\"}
          },
          \"required\": [\"task_id\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "automation_read 工具定义" "pass"
else
  log_test "automation_read 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 5: automation_update（自动化更新）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: automation_update（自动化更新）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"更新自动化任务\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"automation_update\",
        \"description\": \"更新自动化任务\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"task_id\": {\"type\": \"string\", \"description\": \"任务ID\"},
            \"name\": {\"type\": \"string\", \"description\": \"任务名称\"},
            \"description\": {\"type\": \"string\", \"description\": \"任务描述\"}
          },
          \"required\": [\"task_id\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "automation_update 工具定义" "pass"
else
  log_test "automation_update 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 6: automation_run（自动化运行）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: automation_run（自动化运行）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"运行自动化任务\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"automation_run\",
        \"description\": \"运行自动化任务\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"task_id\": {\"type\": \"string\", \"description\": \"任务ID\"}
          },
          \"required\": [\"task_id\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "automation_run 工具定义" "pass"
else
  log_test "automation_run 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 7: handle_read（句柄读取）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: handle_read（句柄读取）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"读取句柄内容\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"handle_read\",
        \"description\": \"读取句柄内容\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"handle_id\": {\"type\": \"string\", \"description\": \"句柄ID\"}
          },
          \"required\": [\"handle_id\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "handle_read 工具定义" "pass"
else
  log_test "handle_read 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 8: retrieve_tool_result（工具结果检索）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: retrieve_tool_result（工具结果检索）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"检索工具执行结果\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"retrieve_tool_result\",
        \"description\": \"检索工具执行结果\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"tool_call_id\": {\"type\": \"string\", \"description\": \"工具调用ID\"}
          },
          \"required\": [\"tool_call_id\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "retrieve_tool_result 工具定义" "pass"
else
  log_test "retrieve_tool_result 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 9: rlm_session_objects（RLM 会话对象）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: rlm_session_objects（RLM 会话对象）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"获取 RLM 会话对象列表\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"rlm_session_objects\",
        \"description\": \"获取 RLM 会话对象列表\",
        \"parameters\": {\"type\": \"object\", \"properties\": {}}
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "rlm_session_objects 工具定义" "pass"
else
  log_test "rlm_session_objects 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 10: rlm_open（RLM 打开会话）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: rlm_open（RLM 打开会话）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"打开 RLM 会话\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"rlm_open\",
        \"description\": \"打开 RLM 会话\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"session_id\": {\"type\": \"string\", \"description\": \"会话ID\"}
          },
          \"required\": [\"session_id\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "rlm_open 工具定义" "pass"
else
  log_test "rlm_open 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 11: rlm_eval（RLM 评估）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: rlm_eval（RLM 评估）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"评估 RLM 会话\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"rlm_eval\",
        \"description\": \"评估 RLM 会话\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"session_id\": {\"type\": \"string\", \"description\": \"会话ID\"},
            \"expression\": {\"type\": \"string\", \"description\": \"评估表达式\"}
          },
          \"required\": [\"session_id\", \"expression\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "rlm_eval 工具定义" "pass"
else
  log_test "rlm_eval 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 12: rlm_configure（RLM 配置）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: rlm_configure（RLM 配置）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"配置 RLM 会话\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"rlm_configure\",
        \"description\": \"配置 RLM 会话\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"session_id\": {\"type\": \"string\", \"description\": \"会话ID\"},
            \"config\": {\"type\": \"object\", \"description\": \"配置项\"}
          },
          \"required\": [\"session_id\", \"config\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "rlm_configure 工具定义" "pass"
else
  log_test "rlm_configure 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 13: rlm_close（RLM 关闭会话）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: rlm_close（RLM 关闭会话）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"关闭 RLM 会话\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"rlm_close\",
        \"description\": \"关闭 RLM 会话\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"session_id\": {\"type\": \"string\", \"description\": \"会话ID\"}
          },
          \"required\": [\"session_id\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "rlm_close 工具定义" "pass"
else
  log_test "rlm_close 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 14: wait_for_dev_server（开发服务器等待）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: wait_for_dev_server（开发服务器等待）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"等待开发服务器就绪\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"wait_for_dev_server\",
        \"description\": \"等待开发服务器就绪\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"url\": {\"type\": \"string\", \"description\": \"服务器URL\"},
            \"timeout\": {\"type\": \"integer\", \"description\": \"超时时间（秒）\"}
          },
          \"required\": [\"url\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "wait_for_dev_server 工具定义" "pass"
else
  log_test "wait_for_dev_server 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 15: multi_tool_use.parallel（并行多工具）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: multi_tool_use.parallel（并行多工具）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"使用多工具并行执行\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"multi_tool_use.parallel\",
        \"description\": \"并行执行多个工具调用\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"tool_uses\": {
              \"type\": \"array\",
              \"items\": {
                \"type\": \"object\",
                \"properties\": {
                  \"tool\": {\"type\": \"string\"},
                  \"input\": {\"type\": \"object\"}
                }
              },
              \"description\": \"要并行执行的工具列表\"
            }
          },
          \"required\": [\"tool_uses\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "multi_tool_use.parallel 工具定义" "pass"
else
  log_test "multi_tool_use.parallel 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 16: load_skill（技能加载）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: load_skill（技能加载）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"加载技能\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"load_skill\",
        \"description\": \"加载技能文件\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"skill_name\": {\"type\": \"string\", \"description\": \"技能名称\"}
          },
          \"required\": [\"skill_name\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "load_skill 工具定义" "pass"
else
  log_test "load_skill 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 17: pandoc_convert（文档转换）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: pandoc_convert（文档转换）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"转换文档格式\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"pandoc_convert\",
        \"description\": \"使用 pandoc 转换文档格式\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"input_file\": {\"type\": \"string\", \"description\": \"输入文件路径\"},
            \"output_format\": {\"type\": \"string\", \"description\": \"输出格式\"}
          },
          \"required\": [\"input_file\", \"output_format\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "pandoc_convert 工具定义" "pass"
else
  log_test "pandoc_convert 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 18: speech（语音合成）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: speech（语音合成）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"合成语音\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"speech\",
        \"description\": \"将文本转换为语音\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"text\": {\"type\": \"string\", \"description\": \"要转换的文本\"},
            \"voice\": {\"type\": \"string\", \"description\": \"语音类型\"}
          },
          \"required\": [\"text\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "speech 工具定义" "pass"
else
  log_test "speech 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 19: js_execution（JS 执行）
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: js_execution（JS 执行）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"执行 JavaScript 代码\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"js_execution\",
        \"description\": \"执行 JavaScript 代码\",
        \"parameters\": {
          \"type\": \"object\",
          \"properties\": {
            \"code\": {\"type\": \"string\", \"description\": \"要执行的 JavaScript 代码\"}
          },
          \"required\": [\"code\"]
        }
      }
    }]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "js_execution 工具定义" "pass"
else
  log_test "js_execution 工具定义" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 工具 20: 多工具组合测试
# ════════════════════════════════════════════════════════════════
echo -e "${YELLOW}▸ 工具: 多工具组合测试（automation + handle + rlm）${NC}"

response=$(curl -s --max-time 30 "$BASE_URL/chat/completions" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $API_KEY" \
  -d "{
    \"model\": \"$MODEL\",
    \"messages\": [{\"role\": \"user\", \"content\": \"创建自动化任务，读取句柄，配置 RLM 会话\"}],
    \"tools\": [
      {
        \"type\": \"function\",
        \"function\": {
          \"name\": \"automation_create\",
          \"description\": \"创建自动化任务\",
          \"parameters\": {\"type\": \"object\", \"properties\": {\"name\": {\"type\": \"string\"}, \"description\": {\"type\": \"string\"}}, \"required\": [\"name\", \"description\"]}
        }
      },
      {
        \"type\": \"function\",
        \"function\": {
          \"name\": \"handle_read\",
          \"description\": \"读取句柄内容\",
          \"parameters\": {\"type\": \"object\", \"properties\": {\"handle_id\": {\"type\": \"string\"}}, \"required\": [\"handle_id\"]}
        }
      },
      {
        \"type\": \"function\",
        \"function\": {
          \"name\": \"rlm_configure\",
          \"description\": \"配置 RLM 会话\",
          \"parameters\": {\"type\": \"object\", \"properties\": {\"session_id\": {\"type\": \"string\"}, \"config\": {\"type\": \"object\"}}, \"required\": [\"session_id\", \"config\"]}
        }
      }
    ]
  }")

if echo "$response" | python3 -c "import sys,json; d=json.load(sys.stdin); print('ok')" 2>/dev/null | grep -q "ok"; then
  log_test "多工具组合测试" "pass"
else
  log_test "多工具组合测试" "fail"
fi

# ════════════════════════════════════════════════════════════════
# 输出结果摘要
# ════════════════════════════════════════════════════════════════
echo ""
echo -e "${BLUE}══════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}     扩展工具测试结果摘要${NC}"
echo -e "${BLUE}══════════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "  总测试数: $test_number"
echo -e "  ${GREEN}通过: $pass_count${NC}"
echo -e "  ${RED}失败: $fail_count${NC}"
echo ""

if [ $fail_count -gt 0 ]; then
  echo -e "${RED}失败的测试: ${failures[*]}${NC}"
  exit 1
else
  echo -e "${GREEN}✓ 所有扩展工具测试通过！${NC}"
  exit 0
fi
