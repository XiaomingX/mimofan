package blinded;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;

import java.util.List;


















@RestController
public class OrderControllerBy {

    private final ParamDataService dataService = new ParamDataService();

    


    @GetMapping("/benchmark/crosssvc/orders/by")
    public List<String> searchOrders(@RequestParam("filter") String filter) {
        // 下游回传参数化片段（污点仅进绑定参数，不进 SQL 文本）
        ParamDataService.SqlQuery q = dataService.fetchSqlFragment(filter);

        /*ANCHOR_1*/
        return queryForList(q.template, q.param); // 固定模板，filter 仅作绑定参数
    }

    


    static List<String> queryForList(String sql, Object param) {
        System.out.println("[queryForList-by] " + sql + " params=" + param);
        return java.util.Collections.emptyList();
    }

    
    static class ParamDataService {
        
        SqlQuery fetchSqlFragment(String filter) {
            // 语义等价：PreparedStatement "SELECT * FROM orders WHERE status = ?" 绑定 filter
            return new SqlQuery("SELECT * FROM orders WHERE status = ?", filter);
        }

        static class SqlQuery {
            final String template;
            final Object param;
            SqlQuery(String template, Object param) {
                this.template = template;
                this.param = param;
            }
        }
    }
}
