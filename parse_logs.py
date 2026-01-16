#!/usr/bin/env python3
"""
htsim-rs 日志解析器
使用 Rich 库美化 tracing 日志输出
"""

import re
import sys
from datetime import datetime
from typing import Optional, List, Dict
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from rich.text import Text
from rich import box

console = Console()

# ANSI 转义码正则表达式（用于去除颜色代码）
ANSI_ESCAPE = re.compile(r'\x1b\[[0-9;]*m')


def strip_ansi(line: str) -> str:
    """去除 ANSI 转义码（颜色代码）"""
    return ANSI_ESCAPE.sub('', line)

# 日志格式解析正则
# 格式: timestamp LEVEL spans}: module: file:line: message
# spans 可能很复杂，包含多层嵌套的 {...}，最后以 }: 结尾
LOG_PATTERN = re.compile(
    r'(?P<timestamp>\d{4}-\d{2}-\d{2}T[\d:\.]+Z)\s+'
    r'(?P<level>\w+)\s+'
    r'(?P<spans>.+?)(?:\}:\s+|:\s+)(?=htsim_rs::)'  # 匹配到 }: 或 : 后面跟着 htsim_rs::
    r'(?P<module>htsim_rs::[^:]+(?:::[^:]+)?):\s+'  # 模块以 htsim_rs:: 开头
    r'(?P<file>[^:]+):(?P<line>\d+):\s+'
    r'(?P<message>.*)'
)

# 级别颜色映射
LEVEL_COLORS = {
    'ERROR': 'red',
    'WARN': 'yellow',
    'INFO': 'blue',
    'DEBUG': 'cyan',
    'TRACE': 'dim white',
}

# 级别图标
LEVEL_ICONS = {
    'ERROR': '❌',
    'WARN': '⚠️',
    'INFO': 'ℹ️',
    'DEBUG': '🔍',
    'TRACE': '🔎',
}


def parse_log_line(line: str) -> Optional[Dict]:
    """解析单行日志"""
    # 先去除 ANSI 转义码
    line = strip_ansi(line.strip())
    
    match = LOG_PATTERN.match(line)
    if not match:
        return None
    
    spans = match.group('spans')
    # 如果 spans 以 } 结尾但没有 }:，说明需要补上 }
    if spans.endswith('}') and not spans.endswith('}:'):
        spans = spans + ':'
    
    return {
        'timestamp': match.group('timestamp'),
        'level': match.group('level'),
        'spans': spans,
        'module': match.group('module'),
        'file': match.group('file'),
        'line': match.group('line'),
        'message': match.group('message'),
    }


def parse_spans(spans_str: str) -> List[Dict[str, str]]:
    """解析调用链 spans - 简化版本"""
    spans = []
    # 格式: func1{...}:func2:func3{...}
    # 按 }: 或 : 分割，但要小心处理嵌套的 {}
    
    # 先保护模块名中的 ::
    spans_str = spans_str.replace('::', '⦂⦂')
    
    # 使用状态机解析
    current_func = ""
    brace_depth = 0
    i = 0
    
    while i < len(spans_str):
        char = spans_str[i]
        
        if char == '{':
            brace_depth += 1
            if brace_depth == 1:
                # 开始字段部分，保存函数名
                if current_func.strip():
                    spans.append({'function': current_func.strip(), 'fields': {}})
                    current_func = ""
        elif char == '}':
            brace_depth -= 1
            if brace_depth == 0:
                # 字段部分结束，准备下一个函数
                pass
        elif char == ':' and brace_depth == 0:
            # 函数分隔符
            if current_func.strip():
                spans.append({'function': current_func.strip(), 'fields': {}})
                current_func = ""
            # 跳过空格
            i += 1
            while i < len(spans_str) and spans_str[i] == ' ':
                i += 1
            continue
        else:
            if brace_depth == 0:
                current_func += char
        
        i += 1
    
    # 处理最后一个函数
    if current_func.strip():
        spans.append({'function': current_func.strip(), 'fields': {}})
    
    # 恢复 ::
    for span in spans:
        span['function'] = span['function'].replace('⦂⦂', '::')
    
    return spans


def format_timestamp(ts_str: str) -> str:
    """格式化时间戳"""
    try:
        dt = datetime.fromisoformat(ts_str.replace('Z', '+00:00'))
        return dt.strftime('%H:%M:%S.%f')[:-3]  # 保留毫秒
    except:
        return ts_str


def format_file_path(file_path: str) -> str:
    """格式化文件路径，只显示相对路径"""
    if '/' in file_path:
        return file_path.split('/')[-1]
    return file_path


def format_log_entry(log_data: Dict) -> str:
    """格式化单条日志为字符串"""
    level = log_data['level']
    level_color = LEVEL_COLORS.get(level, 'white')
    level_icon = LEVEL_ICONS.get(level, '•')
    
    # 时间戳
    timestamp = format_timestamp(log_data['timestamp'])
    
    # 消息
    message = log_data['message']
    
    # 调用链（spans）
    spans = parse_spans(log_data['spans'])
    span_str = ""
    if spans:
        span_parts = []
        for span in spans:
            func_name = span['function']
            # 简化函数名显示（去掉模块路径，只保留最后一部分）
            if '::' in func_name:
                func_name = func_name.split('::')[-1]
            elif '.' in func_name:
                func_name = func_name.split('.')[-1]
            span_parts.append(f"[cyan]{func_name}[/cyan]")
        span_str = " [dim]→[/dim] ".join(span_parts)
    
    # 文件位置（只显示文件名和行号，不显示模块路径）
    file_path = format_file_path(log_data['file'])
    location = f"[dim]{file_path}:{log_data['line']}[/dim]"
    
    # 构建输出
    lines = []
    header = f"[dim]{timestamp}[/dim] [{level_color}]{level_icon} {level}[/{level_color}]"
    if span_str:
        header += f"\n  {span_str}"
    lines.append(header)
    lines.append(f"  {location}")
    lines.append(f"  {message}")
    
    return "\n".join(lines)


def create_summary_table(logs: List[Dict]) -> Table:
    """创建统计摘要表格"""
    table = Table(title="日志统计", box=box.ROUNDED)
    table.add_column("级别", style="bold")
    table.add_column("数量", justify="right")
    
    level_counts = {}
    for log in logs:
        level = log['level']
        level_counts[level] = level_counts.get(level, 0) + 1
    
    for level in ['ERROR', 'WARN', 'INFO', 'DEBUG', 'TRACE']:
        count = level_counts.get(level, 0)
        if count > 0:
            color = LEVEL_COLORS.get(level, 'white')
            table.add_row(f"[{color}]{level}[/{color}]", str(count))
    
    return table


def main():
    """主函数"""
    logs = []
    lines_read = 0
    
    # 检查 stdin 是否可用
    if sys.stdin.isatty():
        # 如果是交互式终端（没有管道输入），显示帮助信息
        console.print("[yellow]警告: 没有检测到管道输入[/yellow]")
        console.print("[dim]使用方法: RUST_LOG=debug cargo run -- trace-single-packet 2>&1 | python3 parse_logs.py[/dim]")
        return
    
    # 先读取所有日志，再输出标题（避免输出干扰 stdin）
    # 从 stdin 读取日志
    try:
        # 直接迭代 stdin（适用于管道）
        for line in sys.stdin:
            lines_read += 1
            line = line.rstrip('\n\r')  # 只去掉行尾换行符，保留其他空白
            if not line.strip():  # 跳过空行
                continue
            log_data = parse_log_line(line)
            if log_data:
                logs.append(log_data)
    except (KeyboardInterrupt, EOFError):
        # 正常结束
        pass
    except Exception as e:
        # 如果迭代失败，尝试一次性读取
        try:
            # 设置 stdin 为无缓冲模式
            if hasattr(sys.stdin, 'reconfigure'):
                sys.stdin.reconfigure(encoding='utf-8', errors='replace')
            
            content = sys.stdin.read()
            if content:
                for line in content.splitlines():
                    line = line.strip()
                    if line:
                        log_data = parse_log_line(line)
                        if log_data:
                            logs.append(log_data)
        except Exception as e2:
            console.print(f"[red]读取日志失败: {e2}[/red]")
            return
    
    # 读取完成后再输出标题
    console.print("[bold green]htsim-rs 日志解析器[/bold green]\n")
    
    # 调试信息（如果解析失败）
    if not logs:
        if lines_read > 0:
            console.print(f"[yellow]警告: 读取了 {lines_read} 行，但未能解析任何日志[/yellow]")
            console.print("[dim]可能是日志格式不匹配。前3行示例：[/dim]")
            # 尝试重新读取并显示前几行
            try:
                import io
                sys.stdin.seek(0)
                for i, line in enumerate(sys.stdin):
                    if i >= 3:
                        break
                    console.print(f"[dim]  {line.strip()[:100]}[/dim]")
            except:
                pass
        else:
            console.print("[yellow]没有解析到日志[/yellow]")
            console.print("[dim]提示: 确保日志输出被正确重定向到解析器[/dim]")
            console.print("[dim]示例: RUST_LOG=debug cargo run -- trace-single-packet 2>&1 | python3 parse_logs.py[/dim]")
        return
    
    if not logs:
        console.print("[yellow]没有解析到日志[/yellow]")
        console.print("[dim]提示: 确保日志输出被正确重定向到解析器[/dim]")
        console.print("[dim]示例: RUST_LOG=debug cargo run -- trace-single-packet 2>&1 | python3 parse_logs.py[/dim]")
        return
    
    # 日志列表
    logs_text_parts = []
    for i, log_data in enumerate(logs):
        log_entry = format_log_entry(log_data)
        logs_text_parts.append(log_entry)
        
        # 添加分隔线（除了最后一条）
        if i < len(logs) - 1:
            logs_text_parts.append("[dim]" + "─" * 80 + "[/dim]")
    
    logs_text = "\n\n".join(logs_text_parts)
    console.print(Panel(logs_text, title="[bold blue]日志详情[/bold blue]", border_style="blue"))
    
    # 统计摘要（直接输出，不使用 Layout）
    console.print()  # 空行
    summary_table = create_summary_table(logs)
    console.print(summary_table)


if __name__ == "__main__":
    main()
