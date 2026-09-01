package com.jsef.benchmark.sec;

import org.apache.commons.compress.archivers.tar.TarArchiveEntry;
import org.apache.commons.compress.archivers.tar.TarArchiveInputStream;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;

/**
 * JSEF-Benchmark — Tar Slip 解压路径穿越修复 (CWE-22，难度 L2)
 *
 * 修复：目标目录先 toAbsolutePath().normalize()；e.getName() 经 resolve().normalize()
 * 规范化后，拒绝绝对路径，并校验 out.startsWith(target)，确保解压文件落在目标目录内。
 */
public class TarSlipSafe {

    /**
     * 安全：规范化 + 前缀校验 + 拒绝绝对路径/".."，保证文件写入目标目录内。
     */
    static void untar(InputStream in, String dest) throws IOException {
        TarArchiveInputStream tar = new TarArchiveInputStream(in);
        Path target = Paths.get(dest).toAbsolutePath().normalize();
        TarArchiveEntry e;
        while ((e = tar.getNextTarEntry()) != null) {
            Path out = target.resolve(e.getName()).normalize(); // 规范化消除 ..
            if (e.getName().startsWith("/") || !out.startsWith(target)) {
                // 绝对路径或路径穿越到目标目录之外，直接拒绝
                throw new IOException("tar entry escapes destination: " + e.getName());
            }
            // [CHECKPOINT id=JSEF-TARSLIP-001S cwe=22 level=L2 source=tar entry name sink=TarArchiveInputStream entry path traversal write expect=SAFE]
            Files.copy(tar, out); // 已校验 out 在目标目录内，写入安全
        }
    }
}
