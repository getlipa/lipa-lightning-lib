#!/bin/bash

# Define the input files and output json file
INPUT_FILE="test.json"
TIMES_FILE="test_times.json"
OUTPUT_FILE="slack_message.json"
WORKFLOW_URL=""

# Initialize variables
PASSED_TESTS=()
FAILED_TESTS=()
TEST_NAMES=()
TEST_TIMES=()

# Check if a URL parameter is passed
if [ "$#" -eq 1 ]; then
    WORKFLOW_URL="$1"
fi

# Clear the output file if it already exists
> "$OUTPUT_FILE"

# Function to create the header block for the Slack message
create_header_block() {
    cat <<EOF >> "$OUTPUT_FILE"
{
    "blocks": [
        {
            "type": "header",
            "text": {
                "type": "plain_text",
                "text": "3L Monitoring Test Report :test_tube:",
                "emoji": true
            }
        },
        {
            "type": "divider"
        },
EOF
}

# Function to create the summary block for the Slack message
create_summary_block() {
    total_passed=$1
    total_failed=$2

    cat <<EOF >> "$OUTPUT_FILE"
        {
            "type": "section",
            "fields": [
                {
                    "type": "mrkdwn",
                    "text": "*Total Tests Passed:*\n$total_passed"
                },
                {
                    "type": "mrkdwn",
                    "text": "*Total Tests Failed:*\n$total_failed"
                }
            ]
        },
        {
            "type": "divider"
        },
EOF
}

# Function to create the passed tests block
create_passed_tests_block() {
    if [ ${#PASSED_TESTS[@]} -gt 0 ]; then
        echo "        {" >> "$OUTPUT_FILE"
        echo '            "type": "section",' >> "$OUTPUT_FILE"
        echo '            "text": {' >> "$OUTPUT_FILE"
        echo '                "type": "mrkdwn",' >> "$OUTPUT_FILE"
        echo '                "text": "*Passed Tests:*"' >> "$OUTPUT_FILE"
        echo "            }," >> "$OUTPUT_FILE"
        echo '            "fields": [' >> "$OUTPUT_FILE"
        for test in "${PASSED_TESTS[@]}"; do
            echo '                {' >> "$OUTPUT_FILE"
            echo '                    "type": "mrkdwn",' >> "$OUTPUT_FILE"
            echo '                    "text": "`'$test'`"' >> "$OUTPUT_FILE"
            echo '                },' >> "$OUTPUT_FILE"
        done
        echo '            ]' >> "$OUTPUT_FILE"
        echo '        },' >> "$OUTPUT_FILE"
        echo '        {' >> "$OUTPUT_FILE"
        echo '            "type": "divider"' >> "$OUTPUT_FILE"
        echo '        },' >> "$OUTPUT_FILE"
    fi
}

# Function to create the failed tests block
create_failed_tests_block() {
    if [ ${#FAILED_TESTS[@]} -gt 0 ]; then
        echo "        {" >> "$OUTPUT_FILE"
        echo '            "type": "section",' >> "$OUTPUT_FILE"
        echo '            "text": {' >> "$OUTPUT_FILE"
        echo '                "type": "mrkdwn",' >> "$OUTPUT_FILE"
        echo '                "text": "*Failed Tests:*"' >> "$OUTPUT_FILE"
        echo "            }," >> "$OUTPUT_FILE"
        echo '            "fields": [' >> "$OUTPUT_FILE"

        for test in "${FAILED_TESTS[@]}"; do
            test_name=$(echo "$test" | cut -d "|" -f 1)

            # Add the test name as its own field
            echo '                {' >> "$OUTPUT_FILE"
            echo '                    "type": "mrkdwn",' >> "$OUTPUT_FILE"
            echo '                    "text": "`'$test_name'`"' >> "$OUTPUT_FILE"
            echo '                },' >> "$OUTPUT_FILE"
        done

        echo '            ]' >> "$OUTPUT_FILE"
        echo '        },' >> "$OUTPUT_FILE"
    fi
}

# Function to create the test execution times block
create_test_times_block() {
    if [ ${#TEST_NAMES[@]} -gt 0 ]; then
        echo "        {" >> "$OUTPUT_FILE"
        echo '            "type": "section",' >> "$OUTPUT_FILE"
        echo '            "text": {' >> "$OUTPUT_FILE"
        echo '                "type": "mrkdwn",' >> "$OUTPUT_FILE"
        echo '                "text": "*Execution Times:*"' >> "$OUTPUT_FILE"
        echo "            }," >> "$OUTPUT_FILE"
        echo '            "fields": [' >> "$OUTPUT_FILE"
        for i in "${!TEST_NAMES[@]}"; do
            echo '                {' >> "$OUTPUT_FILE"
            echo '                    "type": "mrkdwn",' >> "$OUTPUT_FILE"
            echo '                    "text": "`'${TEST_NAMES[$i]}'`: '${TEST_TIMES[$i]}' seconds"' >> "$OUTPUT_FILE"
            echo '                },' >> "$OUTPUT_FILE"
        done
        echo '            ]' >> "$OUTPUT_FILE"
        echo '        },' >> "$OUTPUT_FILE"
        echo '        {' >> "$OUTPUT_FILE"
        echo '            "type": "divider"' >> "$OUTPUT_FILE"
        echo '        },' >> "$OUTPUT_FILE"
    fi
}

# Function to create the workflow URL block
create_workflow_url_block() {
    if [ -n "$WORKFLOW_URL" ]; then
        cat <<EOF >> "$OUTPUT_FILE"
        {
            "type": "section",
            "text": {
                "type": "mrkdwn",
                "text": "*Workflow URL:*\n<$WORKFLOW_URL>"
            }
        },
EOF
    fi
}

# Function to end the JSON message block
end_json_block() {
    create_workflow_url_block

    echo "    ]" >> "$OUTPUT_FILE"
    echo "}" >> "$OUTPUT_FILE"
}

# Parse the test times JSON file and store in parallel arrays
while IFS= read -r line; do
    test_name=$(echo "$line" | jq -r '.test')
    time_seconds=$(echo "$line" | jq -r '.time_seconds')

    # Store test names and execution times in parallel arrays
    TEST_NAMES+=("$test_name")
    TEST_TIMES+=("$time_seconds")
done < "$TIMES_FILE"

# Parse the test result JSON line by line
while IFS= read -r line; do
    # Parse the type and event fields from the JSON line
    type=$(echo "$line" | jq -r '.type')
    event=$(echo "$line" | jq -r '.event')

    if [[ "$type" == "test" ]]; then
        test_name=$(echo "$line" | jq -r '.name')

        if [[ "$event" == "ok" ]]; then
            PASSED_TESTS+=("$test_name")
        elif [[ "$event" == "failed" ]]; then
            FAILED_TESTS+=("$test_name")
        fi
    elif [[ "$type" == "suite" && ("$event" == "ok" || "$event" == "failed")]]; then
        total_passed=$(echo "$line" | jq -r '.passed')
        total_failed=$(echo "$line" | jq -r '.failed')

        # Create header and summary in the Slack message
        create_header_block
        create_summary_block "$total_passed" "$total_failed"
    fi
done < "$INPUT_FILE"

# Write the Slack message
create_passed_tests_block
create_failed_tests_block
create_test_times_block
end_json_block

echo "Slack Block Kit JSON message generated in $OUTPUT_FILE"
