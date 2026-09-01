package blinded;

import java.io.InputStream;
import java.nio.file.Paths;

/*
 * JSEF-Benchmark L1 — 文件上传无大小限制 (CWE-400)
 *
 * 难度：L1（单跳）。uploadStream 直接 Files.copy 到磁盘，不限制大小，
 * 攻击者可上传超大文件耗尽磁盘/内存。
 *
 * 安全底线：仅 localhost 演示语义。
 * 修复要点（UploadNoSizeLimitBy.java）：边读边计数，超限抛异常。
 */
public class UploadNoSizeLimitBx {

    /*ANCHOR_1*/
    public void upload(InputStream uploadStream) throws Exception {
        // 直接拷贝，不校验大小 → 超大文件耗尽资源
        java.nio.file.Files.copy(uploadStream, Paths.get("/tmp/upload.bin"));
    }
}
