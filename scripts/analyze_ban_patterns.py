#!/usr/bin/env python3
"""
分析封号模式的测试脚本

模拟用户场景：
- 4 个账号
- 每个账号 2 个请求
- 请求间隔 6-10 秒
- 同 IP 注册

验证假设：
1. Fixed window burst at hour boundaries
2. Session create/delete pattern per request
3. Concurrent retry thundering herd
4. Request interval distribution
"""

import asyncio
import json
import random
import time
from dataclasses import dataclass, field
from datetime import datetime
from typing import List, Dict, Any
import statistics


@dataclass
class RequestEvent:
    account_id: str
    timestamp: float
    event_type: str  # "session_create", "session_delete", "request_start", "request_end"
    details: Dict[str, Any] = field(default_factory=dict)


@dataclass
class AccountState:
    account_id: str
    window_start: float = 0
    window_count: int = 0
    total_requests: int = 0
    first_request_time: float = 0
    last_request_time: float = 0
    recent_intervals: List[float] = field(default_factory=list)
    sessions_created: int = 0
    sessions_deleted: int = 0


class SimulatedRateLimiter:
    """模拟当前的 fixed window rate limiter"""
    
    def __init__(self, hourly_quota: int = 60):
        self.hourly_quota = hourly_quota
        self.accounts: Dict[str, AccountState] = {}
    
    def add_account(self, account_id: str):
        self.accounts[account_id] = AccountState(account_id=account_id)
    
    def can_use_account(self, account_id: str, now: float) -> bool:
        acc = self.accounts[account_id]
        if self.hourly_quota == 0:
            return True
        if now - acc.window_start >= 3600:
            # window expired, reset
            acc.window_start = now
            acc.window_count = 0
        return acc.window_count < self.hourly_quota
    
    def record_request(self, account_id: str, now: float) -> int:
        acc = self.accounts[account_id]
        if now - acc.window_start >= 3600:
            acc.window_start = now
            acc.window_count = 0
        acc.window_count += 1
        acc.total_requests += 1
        
        # Record interval
        if acc.last_request_time > 0:
            interval = now - acc.last_request_time
            acc.recent_intervals.append(interval)
            if len(acc.recent_intervals) > 10:
                acc.recent_intervals.pop(0)
        if acc.first_request_time == 0:
            acc.first_request_time = now
        acc.last_request_time = now
        
        return acc.window_count


class SlidingWindowRateLimiter:
    """模拟 sliding window rate limiter (proposed fix)"""
    
    def __init__(self, hourly_quota: int = 60, window_seconds: int = 3600):
        self.hourly_quota = hourly_quota
        self.window_seconds = window_seconds
        self.accounts: Dict[str, List[float]] = {}  # account_id -> list of timestamps
    
    def add_account(self, account_id: str):
        self.accounts[account_id] = []
    
    def can_use_account(self, account_id: str, now: float) -> bool:
        if self.hourly_quota == 0:
            return True
        timestamps = self.accounts[account_id]
        cutoff = now - self.window_seconds
        # Filter to current window
        current_window = [ts for ts in timestamps if ts > cutoff]
        return len(current_window) < self.hourly_quota
    
    def record_request(self, account_id: str, now: float) -> int:
        self.accounts[account_id].append(now)
        cutoff = now - self.window_seconds
        current_window = [ts for ts in self.accounts[account_id] if ts > cutoff]
        return len(current_window)


def simulate_user_scenario():
    """模拟用户报告的场景"""
    print("=" * 80)
    print("模拟用户场景: 4账号, 每账号2请求, 间隔6-10秒")
    print("=" * 80)
    
    # Fixed window simulation
    fixed_limiter = SimulatedRateLimiter(hourly_quota=60)
    for i in range(4):
        fixed_limiter.add_account(f"account_{i}")
    
    # Sliding window simulation
    sliding_limiter = SlidingWindowRateLimiter(hourly_quota=60)
    for i in range(4):
        sliding_limiter.add_account(f"account_{i}")
    
    events: List[RequestEvent] = []
    base_time = time.time()
    
    # Simulate: 4 accounts, 2 requests each, 6-10s delay between requests
    for account_idx in range(4):
        account_id = f"account_{account_idx}"
        for req_idx in range(2):
            delay = random.uniform(6, 10) if req_idx > 0 else 0
            base_time += delay
            
            # Session create
            events.append(RequestEvent(
                account_id=account_id,
                timestamp=base_time,
                event_type="session_create",
                details={"request_num": req_idx + 1}
            ))
            base_time += 0.1  # session create time
            
            # Request
            events.append(RequestEvent(
                account_id=account_id,
                timestamp=base_time,
                event_type="request_start",
                details={"request_num": req_idx + 1}
            ))
            
            # Record in limiters
            fixed_used = fixed_limiter.record_request(account_id, base_time)
            sliding_used = sliding_limiter.record_request(account_id, base_time)
            
            base_time += random.uniform(2, 5)  # request processing time
            
            # Session delete
            events.append(RequestEvent(
                account_id=account_id,
                timestamp=base_time,
                event_type="session_delete",
                details={"request_num": req_idx + 1}
            ))
            
            events.append(RequestEvent(
                account_id=account_id,
                timestamp=base_time,
                event_type="request_end",
                details={"request_num": req_idx + 1}
            ))
            
            print(f"  {account_id} req{req_idx+1}: fixed_used={fixed_used}/60, sliding_used={sliding_used}/60, delay={delay:.1f}s")
    
    # Analyze
    print("\n--- 分析结果 ---")
    print(f"总请求数: {len([e for e in events if e.event_type == 'request_start'])}")
    print(f"总 Session 创建: {len([e for e in events if e.event_type == 'session_create'])}")
    print(f"总 Session 删除: {len([e for e in events if e.event_type == 'session_delete'])}")
    
    # Check if any account hit quota
    for acc_id, acc in fixed_limiter.accounts.items():
        print(f"\n{acc_id}:")
        print(f"  窗口内请求: {acc.window_count}/{fixed_limiter.hourly_quota}")
        print(f"  总请求数: {acc.total_requests}")
        if acc.recent_intervals:
            print(f"  请求间隔: {[f'{x:.1f}s' for x in acc.recent_intervals]}")
            print(f"  平均间隔: {statistics.mean(acc.recent_intervals):.1f}s")
    
    return events, fixed_limiter, sliding_limiter


def simulate_burst_at_hour_boundary():
    """模拟 fixed window 在小时边界的 burst 问题"""
    print("\n" + "=" * 80)
    print("模拟 Fixed Window 小时边界 Burst 问题")
    print("=" * 80)
    
    limiter = SimulatedRateLimiter(hourly_quota=10)
    limiter.add_account("test_account")
    
    # Simulate requests at minute 55-59 of hour 1 (end of window)
    base_time = 3600 * 1 + 55 * 60  # 1 hour 55 minutes
    print(f"\n第 1 小时最后 5 分钟 (配额 10):")
    for i in range(5):
        can_use = limiter.can_use_account("test_account", base_time)
        used = limiter.record_request("test_account", base_time)
        print(f"  t={base_time:.0f}: can_use={can_use}, used={used}/10")
        base_time += 60  # 1 minute
    
    # New hour starts - window resets, can burst again!
    base_time = 3600 * 2 + 1 * 60  # 2 hours 1 minute
    print(f"\n第 2 小时前 5 分钟 (窗口重置):")
    for i in range(5):
        can_use = limiter.can_use_account("test_account", base_time)
        used = limiter.record_request("test_account", base_time)
        print(f"  t={base_time:.0f}: can_use={can_use}, used={used}/10")
        base_time += 60
    
    print("\n>>> 问题: 1小时内实际发了 10 请求，但跨边界 10 分钟内发了 10 请求！")
    
    # Sliding window comparison
    print("\n--- Sliding Window 对比 ---")
    sliding = SlidingWindowRateLimiter(hourly_quota=10)
    sliding.add_account("test_account")
    
    base_time = 3600 * 1 + 55 * 60
    print("第 1 小时最后 5 分钟:")
    for i in range(5):
        can_use = sliding.can_use_account("test_account", base_time)
        used = sliding.record_request("test_account", base_time)
        print(f"  t={base_time:.0f}: can_use={can_use}, used={used}/10")
        base_time += 60
    
    base_time = 3600 * 2 + 1 * 60
    print("第 2 小时前 5 分钟 (滑动窗口包含上小时最后请求):")
    for i in range(5):
        can_use = sliding.can_use_account("test_account", base_time)
        used = sliding.record_request("test_account", base_time)
        print(f"  t={base_time:.0f}: can_use={can_use}, used={used}/10")
        base_time += 60
    
    print(">>> 滑动窗口自然平滑过渡，无 burst")


def simulate_concurrent_retry_thundering_herd():
    """模拟并发重试导致的 thundering herd"""
    print("\n" + "=" * 80)
    print("模拟 429 触发的并发重试 (Thundering Herd)")
    print("=" * 80)
    
    # Current behavior: all accounts retry independently
    accounts = [f"account_{i}" for i in range(4)]
    base_time = time.time()
    
    print("\n当前行为 (独立重试):")
    for attempt in range(3):
        print(f"  尝试 {attempt + 1}:")
        for acc in accounts:
            # All 4 accounts retry at roughly same time
            retry_time = base_time + random.uniform(0, 0.1)
            print(f"    {acc} @ {retry_time:.3f}")
        base_time += 2 ** attempt  # exponential backoff
    
    # Proposed: coordinated retry (recovery episode)
    print("\n提议行为 (Coordinated Retry - Recovery Episode):")
    base_time = time.time()
    for attempt in range(3):
        print(f"  尝试 {attempt + 1}:")
        if attempt == 0:
            # First attempt: all try
            for acc in accounts:
                print(f"    {acc} @ {base_time:.3f}")
        else:
            # Subsequent: only 1 probe, others wait
            probe = accounts[0]
            print(f"    {probe} (probe) @ {base_time:.3f}")
            for acc in accounts[1:]:
                print(f"    {acc} (waiting for probe result)")
        base_time += 2 ** attempt
    
    print("\n>>> Coordinated retry 减少 75% 的并发重试请求")


def analyze_session_lifecycle():
    """分析 Session 生命周期模式"""
    print("\n" + "=" * 80)
    print("Session 生命周期模式分析")
    print("=" * 80)
    
    # Current: create -> request -> delete per request
    print("\n当前模式 (每请求创建/删除 Session):")
    print("  Request 1: create_session -> completion -> delete_session")
    print("  Request 2: create_session -> completion -> delete_session")
    print("  Request 3: create_session -> completion -> delete_session")
    print("  Pattern: 短周期重复，上游可见 create/delete 高频交替")
    
    # Proposed: delayed delete + reuse
    print("\n提议模式 (Delayed Delete + Reuse):")
    print("  Request 1: create_session -> completion -> (keep session)")
    print("  Request 2: reuse_session -> completion -> (keep session)")
    print("  Request 3: reuse_session -> completion -> (keep session)")
    print("  ... 5-10 分钟后 delete_session")
    print("  Pattern: 长周期复用，符合正常用户行为")


def main():
    print("DeepSeek 封号模式分析工具")
    print("基于用户报告: 4账号, 独特device_id, 2请求/账号, 6-10秒间隔, 同IP注册")
    
    simulate_user_scenario()
    simulate_burst_at_hour_boundary()
    simulate_concurrent_retry_thundering_herd()
    analyze_session_lifecycle()
    
    print("\n" + "=" * 80)
    print("结论与建议")
    print("=" * 80)
    print("""
1. Fixed Window Rate Limiter -> 改为 Sliding Window
   - 避免小时边界 burst
   - 请求分布更平滑

2. Coordinated Retry (Recovery Episode)
   - 429/5xx 时只发 1 个 probe
   - 其他请求等待结果
   - 防止 thundering herd 触发风控

3. Delayed Session Delete + Reuse
   - Session 复用 5-10 分钟
   - 减少 create/delete 频率
   - 行为更接近真实用户

4. Aggressive Jitter
   - 请求间随机延迟 ±30%
   - 避免固定间隔模式

5. Device Fingerprint Diversity
   - 确保每账号真正独立的 device_id
   - 不同浏览器配置文件
   - 不同 IP (如可能)
""")


if __name__ == "__main__":
    main()
