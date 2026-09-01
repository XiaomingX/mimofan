// [VULN]
package com.jsef.benchmark.vuln;

import org.apache.commons.compress.archivers.tar.TarArchiveEntry;
import org.apache.commons.compress.archivers.tar.TarArchiveInputStream;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;

/**
 * JSEF-Benchmark — Tar Slip 解压路径穿越 (CWE-22，难度 L2)
 *
 * 危险入口：解压时直接用 e.getName() 拼目标路径，未规范化、未校验 ".." 与绝对路径，
 * 攻击者在 tar 中放置 ../../tmp/payload 可覆盖写到目标目录之外（zip-slip 的 tar 变体）。
 *
 * 安全底线：Payload 仅 localhost 演示语义，不提供真实恶意 tar 内容。
 * 修复要点（TarSlipSafe.java）：e.getName() 规范化 + startsWith(dest) 前缀校验。
 */
public class TarSlipVuln {

    /**
     * 危险：e.getName() 直接拼路径并写入，无规范化/前缀校验，可路径穿越覆盖写目录外文件。
     */
    static void untar(InputStream in, String dest) throws IOException {
        // 库行为声明：Apache Commons Compress 的 TAR 流读取器
        TarArchiveInputStream tar = new TarArchiveInputStream(in);
        TarArchiveEntry e;
        while ((e = tar.getNextTarEntry()) != null) {
            Path out = Paths.get(dest, e.getName()); // e.getName() 未校验，可含 ../ 或绝对路径
            // [CHECKPOINT id=JSEF-TARSLIP-001 cwe=22 level=L2 source=tar entry name sink=TarArchiveInputStream entry path traversal write expect=VULN]
            Files.copy(tar, out); // 写入 out：穿越目录时覆盖写目标目录之外的文件
        }
    }
}
