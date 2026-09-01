package blinded;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;

import java.util.List;


























@RestController
public class OrderController {

    private final DataService dataService = new DataService();

    


    @GetMapping("/benchmark/crosssvc/orders")
    public List<String> searchOrders(@RequestParam("filter") String filter) {
        // 入口：不可信 filter 进入链路
        // 中间节点：出站调用下游服务，回传带污点的 SQL 片段（见 DataService.java:33）
        String sqlFragment = dataService.fetchSqlFragment(filter);

        /*ANCHOR_1*/
        return queryForList(sqlFragment); // 污点跨 HTTP 边界回传后拼入查询
    }

    


    static List<String> queryForList(String sql) {
        // 语义等价：jdbcTemplate.queryForList(sql, String.class)
        System.out.println("[queryForList] " + sql);
        return java.util.Collections.emptyList();
    }
}
